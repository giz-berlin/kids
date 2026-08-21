use crate::target::interface;
use crate::target::synapse::{dto, external};
use crate::{error, source, types};
use std::collections;

#[derive(serde::Deserialize)]
pub struct SynapseConfig {
    pub synapse_api: external::SynapseApiConfig,

    /// Only source groups that have a attribute with this name set will be synced as rooms to
    /// Matrix.
    pub source_room_name_attr: String,
    /// How should the syncer react to rooms that are should be deleted?
    /// Note that this not only happens when the corresponding group is deleted in the source,
    /// but also when a group no longer has an attribute named like the value of
    /// `source_room_name_attr` (see above).
    pub room_deletion_strategy: RoomDeletionStrategy,
}

#[derive(serde::Deserialize, Debug, Clone, Copy)]
pub enum RoomDeletionStrategy {
    /// Do not modify the Matrix room. No users will be removed from it
    /// and no attributes (name, alias, power levels) will be updated.
    Ignore,
    /// Kick all users except the sync user from the room. The room will continue to exist and
    /// users may be added again later. Depending on the Matrix homeserver configuration, users
    /// might access messages again if they could read/decrypt them before they were kicked.
    KickAll,
    /// Like [KickAll](RoomDeletionStrategy::KickAll), but the sync user also leaves the room.
    /// With no members left, it is **impossible to re-add members**, the room is "bricked".
    /// Synapse will shut down the room after some time and might delete messages from the database.
    Evacuate,
    /// The most effective and dangerous option. The room is completely deleted and all traces of
    /// it are removed from the database immediately.
    Delete,
}

// If the source_room_name_attr matches this value, instead of using its content as the
// display name directly, derive the display name of a room from the name of a source group
// by replacing all _ and - with spaces.
const DERIVE_DISPLAY_NAME_FROM_GROUP_NAME: &str = "_name_titlecase";

/// A connector to Synapse providing the [Target](interface::Target) interface.
///
/// NOTE: We assume that all administrative changes to the Synapse are performed automatically.
/// If an administrator were to manually perform certain actions (for example, change the
/// mapping of a Synapse room to a source group), this might lead to undefined behavior such
/// as the Syncer creating a second room for the same group, etc.
/// Being able to fix such a possibly corrupt state automatically under consideration of all
/// edge cases is out of the scope of this implementation.
pub struct Connector {
    config: SynapseConfig,
    synapse_api: Box<dyn external::SynapseApi + Send + Sync>,
    group_id_mapping: Option<collections::HashMap<types::SharedResourceIdentifier, String>>,
    user_id_mapping: Option<collections::HashMap<types::SharedResourceIdentifier, dto::User>>,
}

impl Connector {
    /// This function generates different id mappings required as user and group ids in
    /// Keycloak are different than in Synapse.
    ///
    /// Therefore, we need a mapping between both, which is built in this function.
    async fn generate_id_mappings(&mut self) -> Result<(), error::KidsError> {
        let matrix_rooms = self
            .synapse_api
            .get_joined_rooms_of_syncer()
            .await
            .map_err(|e| e.with_context("Failed getting rooms syncer has joined"))?;

        self.migrate(&matrix_rooms.joined_rooms).await;

        let mut new_group_id_mapping = collections::HashMap::new();

        for matrix_room_id in matrix_rooms.joined_rooms {
            // If this request fails, we don't want to abort the whole method as it only affects a single room.
            // Note that in this case, we might actually create a second room for the same group (if there exists a room mapped to the group
            // in Synapse already, but we failed to obtain that mapping).
            let source_group_id = match self.synapse_api.get_room_associated_source_group_id(&matrix_room_id).await {
                Ok(source_group_id) => source_group_id,
                Err(error) => {
                    // To reach eventual consistency, we need to delete all rooms the syncer is in
                    // that have no source groups associated to them.
                    // This is because such a situation might happen when creation of a room succeeds, but
                    // the subsequent request to associate the source ID fails. In that case, the syncer would retry
                    // creating a room for the associated group, but that would fail because the desired alias
                    // of that room would clash with the one previously created.
                    if let error::KidsError::ApiOperationFailed(_, 404, ..) = error {
                        tracing::error!(
                            matrix_room_id,
                            "Encountered a room the syncer has joined that has no source group associated to it. Deleting that room"
                        );
                        if let Err(e) = self.delete_room(&matrix_room_id, RoomDeletionStrategy::Delete).await {
                            tracing::warn!(matrix_room_id, error=%e, "Could not delete room with no associated source group id");
                        }
                    } else {
                        tracing::error!(?error, matrix_room_id, "Could not determine source group for room");
                    }

                    continue;
                }
            };
            if new_group_id_mapping.contains_key(&source_group_id) {
                tracing::warn!(
                    source_group_id,
                    first_room_id = matrix_room_id,
                    second_room_id = new_group_id_mapping[&source_group_id],
                    "Found duplicate mapping for source group"
                );
                // We don't really know which room really is the better one to use in case of duplicate mapping,
                // so might as well go with the first one we already encountered earlier.
                continue;
            }

            new_group_id_mapping.insert(source_group_id, matrix_room_id);
        }
        self.group_id_mapping = Some(new_group_id_mapping);

        let matrix_users = self.synapse_api.get_users().await.map_err(|e| e.with_context("Failed getting matrix users"))?;

        let mut new_user_id_mapping: collections::HashMap<String, dto::User> = collections::HashMap::new();
        for user in matrix_users.users {
            let source_user_id = match self.synapse_api.get_source_user_id_for_matrix_user_id(&user.name).await {
                Ok(source_user_id) => source_user_id,
                Err(error) => {
                    tracing::warn!(%error, matrix_user_id=user.name, "Could not obtain source user ID for matrix user");
                    continue;
                }
            };
            if new_user_id_mapping.contains_key(&source_user_id) {
                // This should not happen because matrix does not allow creating two users with equal source ids
                // (otherwise, when logging in via SSO, matrix would not know which user to login).
                tracing::error!(
                    source_user_id,
                    first_matrix_user_id = user.name,
                    second_matrix_user_id = new_user_id_mapping[&source_user_id].name,
                    "Found duplicate mapping for source user"
                );
                continue;
            }

            new_user_id_mapping.insert(source_user_id, user);
        }
        self.user_id_mapping = Some(new_user_id_mapping);

        Ok(())
    }

    async fn get_group_id_mapping(&mut self) -> Result<&mut collections::HashMap<types::SharedResourceIdentifier, String>, error::KidsError> {
        if self.group_id_mapping.is_none() {
            self.generate_id_mappings().await?;
        }

        Ok(self
            .group_id_mapping
            .as_mut()
            .expect("`generate_id_mappings` should have filled in `group_id_mapping`"))
    }

    async fn get_user_id_mapping(&mut self) -> Result<&mut collections::HashMap<types::SharedResourceIdentifier, dto::User>, error::KidsError> {
        if self.user_id_mapping.is_none() {
            self.generate_id_mappings().await?;
        }

        Ok(self
            .user_id_mapping
            .as_mut()
            .expect("`generate_id_mappings` should have filled in `user_id_mapping`"))
    }

    async fn ensure_user_display_name(
        &mut self,
        matrix_user_id: &str,
        source_user: &(dyn source::interface::User + Send + Sync),
    ) -> Result<(), error::KidsError> {
        let matrix_display_name = self.synapse_api.get_user_display_name(matrix_user_id).await?;
        if matrix_display_name != source_user.display_name() {
            tracing::debug!(
                matrix_user_id = matrix_user_id,
                user_id = source_user.id(),
                old_display_name = matrix_display_name,
                new_display_name = source_user.display_name(),
                "Updating user's display name."
            );
            if let Some(username) = source_user.display_name() {
                self.synapse_api.set_user_display_name(matrix_user_id, username.as_str()).await?;
            } else {
                const ERROR_CONTEXT: &str = "Creating or updating user";
                const ERROR_MSG: &str = "Requested to unset the display name of a user (the username is unset). This is impossible in Matrix.";
                tracing::error!(user_id = source_user.id(), "{ERROR_CONTEXT}: {ERROR_MSG}");
                return Err(error::KidsError::RequestFailed(ERROR_CONTEXT.to_owned(), anyhow::anyhow!("{ERROR_MSG}")));
            }
        }
        Ok(())
    }

    async fn ensure_user_email(&mut self, matrix_user_id: &str, source_user: &(dyn source::interface::User + Send + Sync)) -> Result<(), error::KidsError> {
        let matrix_three_pids = self.synapse_api.get_user_three_pids(matrix_user_id).await?;
        let desired_three_pids: &[dto::ThreePID] = if let Some(email) = source_user.email() {
            &[dto::ThreePID {
                medium: dto::ThreePIDMedium::Email,
                address: email.to_owned(),
            }]
        } else {
            &[]
        };
        if matrix_three_pids != desired_three_pids {
            tracing::debug!(
                matrix_user_id = matrix_user_id,
                user_id = source_user.id(),
                old_three_pids = ?matrix_three_pids,
                new_three_pids = ?desired_three_pids,
                "Updating user's 3PIDs."
            );
            self.synapse_api.set_user_three_pids(matrix_user_id, desired_three_pids).await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl interface::Target for Connector {
    type Config = SynapseConfig;

    async fn new(config: Self::Config) -> Result<Self, error::KidsError> {
        let synapse_api = Box::new(
            external::SynapseClient::new(config.synapse_api.clone())
                .await
                .map_err(|e| e.with_context("Failed to create Synapse API client"))?,
        );
        Ok(Connector {
            config,
            synapse_api,
            group_id_mapping: None,
            user_id_mapping: None,
        })
    }

    fn info(&self) -> String {
        "Synapse Connector!".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), error::KidsError> {
        tracing::info!(
            "To prepare for full sync, cleaning mapping between source group IDs and matrix room IDs, as well as source user IDs and matrix user IDs"
        );
        self.group_id_mapping = None;
        self.user_id_mapping = None;

        Ok(())
    }

    /// Return the identifiers of all [Source Groups](source::interface::Group) known to Synapse.
    /// These are exactly the ones we managed to obtain a mapping to a Matrix room for earlier.
    /// There might be additional rooms in Synapse not mapped to a Source group, which will not be considered in the result of this method.
    async fn all_groups(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.get_group_id_mapping().await?.keys().cloned().collect())
    }

    async fn all_users(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.get_user_id_mapping().await?.keys().cloned().collect())
    }

    async fn delete_group(&mut self, source_group_id: &types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        let matrix_room_id = match self.get_group_id_mapping().await?.get(source_group_id) {
            Some(matrix_room) => matrix_room.clone(),
            None => {
                // Note: Since rooms are being created before users, all valid rooms must be contained
                // in the mapping at this point.
                tracing::warn!(
                    source_group_id,
                    "Source group has no known associated room in Synapse that could be deleted. Nothing to be done"
                );
                return Ok(());
            }
        };

        tracing::info!(matrix_room_id, "Deleting room with strategy {:?}", self.config.room_deletion_strategy);

        self.delete_room(&matrix_room_id.clone(), self.config.room_deletion_strategy).await?;

        self.get_group_id_mapping().await?.remove(source_group_id);

        Ok(())
    }

    async fn delete_user(&mut self, user_id: &types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        // Synapse does not support deleting users.
        // Instead, we can only deactivate them, which will revoke all user sessions and prevent
        // the user from logging in again.
        // It will also remove the user from all of their rooms and (if we tell it to do so) erase
        // information such as the display name of the user. It will, however, NOT delete the user
        // or its messages from the database.

        // This means that the user could IN THEORY be reactivated again later, which will
        // NOT allow the user to access their old messages, but other users will be informed that it is the same user.
        // On the other hand, IN PRACTICE, the user was probably properly deleted in the source,
        // so recreating it will actually create a new user in the source, and the user will then
        // also register as a new user in the Synapse.

        let matrix_user = match self.get_user_id_mapping().await?.get(user_id) {
            Some(matrix_user) => matrix_user,
            None => {
                // This should not happen, as the controller should only attempt to delete users that
                // we told it exists in Matrix before via the `self.all_users` method.
                tracing::warn!(source_user_id = user_id, "Cannot deactivate source user, because it is not known to Matrix");
                return Ok(());
            }
        };

        let matrix_user_id = matrix_user.name.clone();
        tracing::info!(matrix_user_id, "Deactivating matrix user");
        self.synapse_api
            .deactivate_user(&matrix_user_id)
            .await
            .map_err(|e| e.with_context(&format!("Could not deactivate matrix user {matrix_user_id}")))?;
        self.get_user_id_mapping().await?.remove(user_id);
        Ok(())
    }

    async fn create_or_update_group(&mut self, source_group: std::sync::Arc<dyn source::interface::Group + Send + Sync>) -> Result<(), error::KidsError> {
        // Note that groups containing the below-mentioned characters will lead to ambiguitive group paths,
        // which is why we do not allow them.
        // For example, a subgroup "B" of group "A" will receive the path "/A/B", but so will a group named "A/B" directly.
        // The colon causes issues because it is used as a delimiter in the matrix room alias.
        if source_group.name().contains(":") || source_group.name().contains("/") {
            return Err(error::KidsError::InternalError(format!(
                "Could not create room for group {}: group name contains invalid character",
                source_group.id()
            )));
        }

        // The target does only care about groups with the domain-specific attribute.
        if !source_group.attributes().contains_key(&self.config.source_room_name_attr) {
            match self.get_group_id_mapping().await?.get(source_group.id()) {
                Some(matrix_room_id) => {
                    tracing::warn!(
                        source_group_id = source_group.id(),
                        matrix_room_id,
                        "The source_room_name_attr has been removed from a group that already had a corresponding Matrix room. Deleting that room now"
                    );
                    // Note that, even though we are in the create_or_update method, we have to delete the group here.
                    // This is because the source_room_name_attr is target-specific and the source knows nothing about it;
                    // in fact, the group will still be present in the source after this even though we are deleting the room
                    // because the attribute is missing.
                    // For this reason, the controller will not call the delete_group method in that case.
                    self.delete_group(source_group.id()).await?;
                }
                None => {
                    tracing::info!(
                        source_group_id = source_group.id(),
                        "Not creating room for group because it does not have the source_room_name_attr {}",
                        self.config.source_room_name_attr
                    );
                }
            }

            // This is not an error condition: We did succeed in performing the requested operation, it's
            // just that we do not want to create a room for that group.
            return Ok(());
        }

        if source_group.attributes()[&self.config.source_room_name_attr].len() > 1 {
            tracing::warn!(
                "Encountered multiple values for source_room_name_attr {}. Will only consider the first one for the room name",
                self.config.source_room_name_attr
            );
        }

        let matrix_room_id = self.get_or_create_room(&source_group).await?;
        self.update_display_name(&matrix_room_id, &source_group).await;
        self.update_canonical_alias(&matrix_room_id, &source_group).await;

        Ok(())
    }

    async fn create_or_update_user(&mut self, source_user: std::sync::Arc<dyn source::interface::User + Send + Sync>) -> Result<(), error::KidsError> {
        let matrix_user = match self.get_user_id_mapping().await?.get(source_user.id()) {
            Some(matrix_user) => matrix_user,
            None => {
                tracing::debug!(
                    source_user_id = source_user.id(),
                    "Source user is not known to Matrix. Before the syncer can handle them, they need to login to Matrix manually first"
                );
                // This is not an error condition: The syncer is only supposed to handle users which have logged in before.
                // We are not able to create users directly from the syncer.
                return Ok(());
            }
        };

        let matrix_user_id = matrix_user.name.clone();
        // Need to fetch this already here as we need to drop the mutable borrow.
        let is_matrix_user_locked = matrix_user.locked;

        self.ensure_user_display_name(matrix_user_id.as_str(), source_user.as_ref()).await?;
        self.ensure_user_email(matrix_user_id.as_str(), source_user.as_ref()).await?;

        let desired_user_groups = source_user
            .groups(true)
            .await
            .map_err(|e| e.with_context(&format!("Could not get source groups associated with source user {}", source_user.id())))?;
        let current_user_rooms = self
            .synapse_api
            .get_user_joined_rooms(&matrix_user_id)
            .await
            .map_err(|e| e.with_context(&format!("Could not get matrix rooms user {matrix_user_id} has currently joined")))?;

        // clone the mapping to prevent lifetime issues, this is not the most efficient, but most readable solution
        let desired_user_rooms_group_id_mappings = self.get_group_id_mapping().await?.clone();

        let mut desired_user_rooms: Vec<String> = desired_user_groups
            .iter()
            .filter_map(|group| {
                // We only want to add the user to groups that have a corresponding matrix room.
                // Note: Since rooms are being created before users, all valid rooms must be contained
                // in the mapping at this point.
                desired_user_rooms_group_id_mappings.get(group.id()).cloned()
            })
            .collect();

        if !source_user.enabled() {
            // If user is not enabled, we want to remove it from all rooms it is in.
            // Simply clearing the desired rooms will have this effect using the logic below.
            desired_user_rooms = vec![];

            if !is_matrix_user_locked {
                // Note that we explicitly want to lock users here, NOT deactivate them.
                // Deactivating users appears to delete all keys of that user, so even when a
                // user is reactivated, they cannot log in with the same identity and lose
                // all of their direct message rooms.
                // With locking, this works properly and unlocked users will encounter the same
                // state they left off with before being locked.
                match self.synapse_api.lock_user(&matrix_user_id).await {
                    Ok(()) => {
                        let user = self.get_user_id_mapping().await?.get_mut(source_user.id()).unwrap();
                        user.locked = true;
                        tracing::info!(matrix_user_id, "Locked user");
                    }
                    Err(e) => tracing::warn!(?e, matrix_user_id, "Could not lock user"),
                };
            }
        }
        if source_user.enabled() && is_matrix_user_locked {
            match self.synapse_api.unlock_user(&matrix_user_id).await {
                Ok(()) => {
                    let user = self.get_user_id_mapping().await?.get_mut(source_user.id()).unwrap();
                    user.locked = false;
                    tracing::info!(matrix_user_id, "Unlocked user");
                }
                Err(e) => tracing::warn!(?e, matrix_user_id, "Could not unlock user"),
            };
        }

        // Add user to all desired groups that they are not already joined to.
        for matrix_room_id in &desired_user_rooms {
            if !current_user_rooms.joined_rooms.contains(matrix_room_id) {
                match self.synapse_api.join_user_to_room(matrix_room_id, &matrix_user_id).await {
                    Ok(()) => tracing::info!(matrix_room_id, matrix_user_id, "User joined matrix room"),
                    Err(e) => tracing::warn!(?e, matrix_room_id, matrix_user_id, "Could not join user to matrix room"),
                }
            } else {
                tracing::trace!(matrix_room_id, matrix_user_id, "User has already joined matrix room");
            }
        }

        // Remove user from all joined groups that are no longer desired.
        for matrix_room_id in &current_user_rooms.joined_rooms {
            if !desired_user_rooms.contains(matrix_room_id) {
                match self.synapse_api.kick_user_from_room(matrix_room_id, &matrix_user_id).await {
                    Ok(()) => tracing::info!(matrix_room_id, matrix_user_id, "User kicked from matrix room"),
                    Err(e) => tracing::warn!(?e, matrix_room_id, matrix_user_id, "Could not kick user from matrix room"),
                };
            } else {
                tracing::trace!(matrix_room_id, matrix_user_id, "User stays in matrix room");
            }
        }

        Ok(())
    }
}

impl Connector {
    // The old syncer used a different event to associate matrix rooms to keycloak rooms
    // Once the new syncer was successfully run once, we should be able to delete this method.
    async fn migrate(&mut self, rooms: &Vec<String>) {
        for room in rooms {
            if let Ok(source_id) = self.synapse_api.get_room_associated_source_group_id_v1(room).await {
                match self.synapse_api.associate_source_group_id_to_room(room, &source_id).await {
                    Ok(()) => tracing::info!(room, "Migrated room"),
                    Err(e) => tracing::warn!(?e, room, "Failed to migrate room"),
                };
            }
        }
    }

    async fn get_or_create_room(&mut self, source_group: &std::sync::Arc<dyn source::interface::Group + Send + Sync>) -> Result<String, error::KidsError> {
        let matrix_room_id = match self.get_group_id_mapping().await?.get(source_group.id()) {
            Some(matrix_room_id) => {
                tracing::debug!(source_id = source_group.id(), "Room already exists");
                matrix_room_id.clone()
            }
            None => {
                tracing::info!(
                    source_group_id = source_group.id(),
                    source_group_name = source_group.name(),
                    "Creating room for group"
                );
                let room_creation_response = self
                    .synapse_api
                    .create_room(source_group.name(), source_group.path())
                    .await
                    .map_err(|e| e.with_context("Could not create room"))?;
                let matrix_room_id = room_creation_response.room_id;
                self.synapse_api
                    .associate_source_group_id_to_room(&matrix_room_id, source_group.id())
                    .await
                    .map_err(|e| e.with_context(&format!("Could not associate source group id {} to room {}", source_group.id(), matrix_room_id)))?;
                self.get_group_id_mapping().await?.insert(source_group.id().to_owned(), matrix_room_id.clone());
                tracing::info!(source_id = source_group.id(), group_name = source_group.name(), matrix_room_id, "Room created");
                matrix_room_id
            }
        };
        Ok(matrix_room_id)
    }

    /// Update the display name of the room to match the one specified by the source group.
    ///
    /// This method expects the self.config.source_room_name_attr to be set on the source group.
    /// It should only be called on groups were that's the case (it will panic otherwise).
    async fn update_display_name(&mut self, matrix_room_id: &str, source_group: &std::sync::Arc<dyn source::interface::Group + Send + Sync>) {
        let old_display_name = self.synapse_api.get_room_display_name(matrix_room_id).await;
        match old_display_name {
            Ok(old_display_name) => {
                let mut new_display_name = source_group.attributes()[&self.config.source_room_name_attr]
                    .first()
                    // We can unwrap here because we only process groups that have that attribute set
                    // when we call this method from create_or_update_group().
                    .expect("The `self.config.source_room_name_attr` must be set on the source group when calling this method")
                    .to_owned();
                if new_display_name == DERIVE_DISPLAY_NAME_FROM_GROUP_NAME {
                    new_display_name = source_group.name().replace("_", " ").replace("-", " ");
                }
                if new_display_name != old_display_name {
                    match self.synapse_api.set_room_display_name(matrix_room_id, &new_display_name).await {
                        Ok(()) => tracing::debug!(old_display_name, new_display_name, matrix_room_id, "Updated display name of room"),
                        Err(e) => tracing::warn!(?e, matrix_room_id, "Could not update display name of room"),
                    }
                }
            }
            Err(e) => tracing::warn!(?e, matrix_room_id, "Could not load display name of room"),
        }
    }

    async fn update_canonical_alias(&mut self, matrix_room_id: &str, source_group: &std::sync::Arc<dyn source::interface::Group + Send + Sync>) {
        let full_room_alias = self.synapse_api.full_room_alias(source_group.path());
        let canonical_alias_event = self.synapse_api.get_room_canonical_alias(matrix_room_id).await;
        match canonical_alias_event {
            Ok(canonical_alias_event) => {
                tracing::trace!(?canonical_alias_event, matrix_room_id, "Found canonical alias for room");

                if canonical_alias_event.alias == full_room_alias {
                    return;
                }

                match self.synapse_api.create_room_alias(matrix_room_id, &full_room_alias).await {
                    Ok(()) => tracing::debug!(matrix_room_id, full_room_alias, "Created new room alias"),
                    Err(e) => {
                        tracing::warn!(?e, matrix_room_id, "Could not create new alias for room. Aborting update of canonical alias");
                        return;
                    }
                }

                match self.synapse_api.set_room_canonical_alias(matrix_room_id, &full_room_alias).await {
                    Ok(()) => tracing::debug!(matrix_room_id, full_room_alias, "Updated canonical alias for room"),
                    Err(e) => {
                        tracing::warn!(?e, matrix_room_id, "Could not update canonical alias for room. Retaining old alias");
                        return;
                    }
                }

                // Note: In the very rare case that above request succeeds and below fails, we will "leak"
                // room aliases in the sense that we will forget retrying deletion of this alias on the
                // next invocation of this method.
                // If you notice a room having multiple aliases, this is probably what has happened
                // (every room should, by invariant, only have exactly one alias).
                // We postpone handling of this edge case for now.
                match self.synapse_api.delete_room_alias(&canonical_alias_event.alias).await {
                    Ok(()) => tracing::debug!(matrix_room_id, old_alias = canonical_alias_event.alias, "Deleted canonical alias from room"),
                    Err(e) => tracing::warn!(
                        ?e,
                        matrix_room_id,
                        old_alias = canonical_alias_event.alias,
                        "Could not delete canonical alias for room"
                    ),
                };
            }
            Err(e) => tracing::warn!(?e, matrix_room_id, "Could not determine current room canonical alias"),
        }
    }

    async fn delete_room(&mut self, matrix_room_id: &str, room_deletion_strategy: RoomDeletionStrategy) -> Result<(), error::KidsError> {
        match room_deletion_strategy {
            RoomDeletionStrategy::KickAll | RoomDeletionStrategy::Evacuate => {
                let joined_members = self
                    .synapse_api
                    .get_room_joined_users(matrix_room_id)
                    .await
                    .map_err(|e| e.with_context("Could not get joined users for room deletion. Could not delete room"))?;
                tracing::info!(matrix_room_id, ?joined_members, "Kicking members from room");

                let mut all_kicked = true;
                for member in joined_members.joined.keys() {
                    tracing::debug!(matrix_room_id, member, "Kicking member from room");
                    if self.synapse_api.user_is_matrix_syncer(member) {
                        continue;
                    }
                    if let Err(e) = self.synapse_api.kick_user_from_room(matrix_room_id, member).await {
                        tracing::error!(matrix_room_id, member, error = ?e, "Could not kick member from room");
                        all_kicked = false;
                    }
                }

                if !all_kicked {
                    // Note: Need to return early here because the syncer should only leave the room
                    // if all users have been kicked successfully.
                    return Err(error::KidsError::InternalError(format!(
                        "Could not kick all members from room {matrix_room_id}"
                    )));
                }

                if matches!(room_deletion_strategy, RoomDeletionStrategy::Evacuate) {
                    tracing::info!(matrix_room_id, "Syncer leaving room");
                    self.synapse_api
                        .syncer_leave_room(matrix_room_id)
                        .await
                        .map_err(|e| e.with_context("Could not leave room"))?;
                }
            }
            RoomDeletionStrategy::Delete => {
                tracing::info!(matrix_room_id, "Deleting room");
                self.synapse_api
                    .delete_room(matrix_room_id)
                    .await
                    .map_err(|e| e.with_context("Could not delete room"))?;
            }
            RoomDeletionStrategy::Ignore => {}
        }

        // Note: If strategy is delete, alias has already been deleted with the room.
        if matches!(room_deletion_strategy, RoomDeletionStrategy::Evacuate) {
            // Each room should only have one alias at a time when being managed by the syncer.
            // In order to avoid issues when evacuating a room but trying to recreate it later (which
            // might use the same alias), make sure to disassociate the alias from the old room.
            tracing::info!(matrix_room_id, "Deleting old alias for room");
            let canonical_alias_event = self
                .synapse_api
                .get_room_canonical_alias(matrix_room_id)
                .await
                .map_err(|e| e.with_context("Could not obtain canonical alias for room"))?;
            self.synapse_api
                .delete_room_alias(&canonical_alias_event.alias)
                .await
                .map_err(|e| e.with_context("Could not delete canonical alias for room"))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::source::interface::Group;
    use crate::target::interface::Target;
    use crate::target::synapse::external::MockSynapseApi;
    use crate::target::synapse::test_mocks::{MockSynapseRoomBuilder, MockSynapseUserBuilder, SynapseApiMocker};
    use crate::test_util::constants;
    use rstest::*;

    #[fixture]
    pub fn connector() -> Connector {
        Connector {
            config: SynapseConfig {
                synapse_api: external::SynapseApiConfig {
                    matrix_homeserver_url: "".to_string(),
                    matrix_source_oidc_provider_id: "".to_string(),
                    matrix_syncer_user_id: "".to_string(),
                    matrix_syncer_password: "".to_string(),
                    matrix_namespace: "".to_string(),
                    insecure_disable_tls_verification: true,
                },
                room_deletion_strategy: RoomDeletionStrategy::Ignore,
                source_room_name_attr: "test".to_string(),
            },
            synapse_api: Box::new(MockSynapseApi::default()),
            group_id_mapping: None,
            user_id_mapping: None,
        }
    }

    #[rstest]
    fn info_works(connector: Connector) {
        assert_eq!(connector.info(), "Synapse Connector!")
    }

    mod when_full_sync_incoming_and_generate_id_mappings {
        use super::*;

        #[rstest]
        #[tokio::test]
        async fn then_return_ok(mut connector: Connector) {
            // given
            connector.synapse_api = SynapseApiMocker::new()
                .can_get_joined_rooms_of_syncer()
                .can_associate_source_group_id_to_room()
                .can_get_users()
                .into();

            // when & then
            assert!(connector.full_sync_incoming().await.is_ok());
            assert!(connector.generate_id_mappings().await.is_ok());
        }

        #[rstest]
        #[tokio::test]
        async fn then_add_groups_to_group_mapping(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().build();

            connector.synapse_api = SynapseApiMocker::new()
                .with_rooms(vec![room1.clone(), room2.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_room_associated_source_group_id_v1()
                .can_associate_source_group_id_to_room()
                .can_get_all_rooms_associated_source_group_id()
                .can_get_users()
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert_eq!(connector.get_group_id_mapping().await.unwrap().len(), 2);
            assert_eq!(
                connector.get_group_id_mapping().await.unwrap().get(&room1.source_room_id).unwrap(),
                &room1.matrix_room_id
            );
            assert_eq!(
                connector.get_group_id_mapping().await.unwrap().get(&room2.source_room_id).unwrap(),
                &room2.matrix_room_id
            );
            assert_eq!(
                connector.all_groups().await.unwrap(),
                std::collections::HashSet::from([room1.source_room_id, room2.source_room_id])
            );
        }

        #[rstest]
        #[tokio::test]
        async fn then_add_users_to_user_mapping(mut connector: Connector) {
            // given
            let user1 = MockSynapseUserBuilder::default().build();
            let user2 = MockSynapseUserBuilder::default().build();

            connector.synapse_api = SynapseApiMocker::new()
                .with_users(vec![user1.clone(), user2.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_users()
                .can_get_source_user_id_for_all_matrix_users()
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert_eq!(connector.get_user_id_mapping().await.unwrap().len(), 2);
            assert_eq!(
                connector.get_user_id_mapping().await.unwrap().get(&user1.source_user_id).unwrap(),
                &SynapseApiMocker::get_user_from(&user1)
            );
            assert_eq!(
                connector.get_user_id_mapping().await.unwrap().get(&user2.source_user_id).unwrap(),
                &SynapseApiMocker::get_user_from(&user2)
            );
            assert_eq!(
                connector.all_users().await.unwrap(),
                std::collections::HashSet::from([user1.source_user_id, user2.source_user_id])
            );
        }

        #[rstest]
        #[tokio::test]
        async fn then_completely_clear_mappings_before_rebuild(mut connector: Connector) {
            // given
            connector.synapse_api = SynapseApiMocker::new().can_get_joined_rooms_of_syncer().can_get_users().into();

            connector
                .get_group_id_mapping()
                .await
                .unwrap()
                .insert(constants::DEFAULT_SOURCE_GROUP_ID.to_string(), constants::DEFAULT_TARGET_ROOM_ID.to_string());
            connector.get_user_id_mapping().await.unwrap().insert(
                constants::DEFAULT_SOURCE_USER_ID.to_string(),
                dto::User {
                    name: constants::DEFAULT_TARGET_USER_ID.to_string(),
                    locked: false,
                    external_ids: None,
                    threepids: None,
                },
            );

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert!(connector.get_user_id_mapping().await.unwrap().is_empty());
            assert!(connector.get_group_id_mapping().await.unwrap().is_empty());
        }

        #[rstest]
        #[tokio::test]
        async fn and_mapping_ambiguous_then_use_first_encountered_value(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().source_room_id(room1.source_room_id.clone()).build();

            let user1 = MockSynapseUserBuilder::default().build();
            let user2 = MockSynapseUserBuilder::default().source_user_id(user1.source_user_id.clone()).build();

            connector.synapse_api = SynapseApiMocker::new()
                .with_rooms(vec![room1.clone(), room2.clone()])
                .with_users(vec![user1.clone(), user2.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_room_associated_source_group_id_v1()
                .can_associate_source_group_id_to_room()
                .can_get_all_rooms_associated_source_group_id()
                .can_get_users()
                .can_get_source_user_id_for_all_matrix_users()
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert_eq!(
                connector.get_group_id_mapping().await.unwrap().get(&room1.source_room_id).unwrap(),
                &room1.matrix_room_id
            );
            assert_eq!(
                connector.get_user_id_mapping().await.unwrap().get(&user1.source_user_id).unwrap(),
                &SynapseApiMocker::get_user_from(&user1)
            );
        }

        #[rstest]
        #[tokio::test]
        async fn and_obtaining_mapping_for_one_room_fails_then_still_process_other_rooms(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().build();
            let room3 = MockSynapseRoomBuilder::default().build();

            connector.synapse_api = SynapseApiMocker::new()
                .with_rooms(vec![room1.clone(), room2.clone(), room3.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_room_associated_source_group_id_v1()
                .can_associate_source_group_id_to_room()
                .can_get_room_associated_source_group_id_for_room(&room1)
                .cannot_get_room_associated_source_group_id_for_room(&room2)
                .can_get_room_associated_source_group_id_for_room(&room3)
                .can_get_users()
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert!(connector.get_group_id_mapping().await.unwrap().contains_key(&room1.source_room_id));
            assert!(!connector.get_group_id_mapping().await.unwrap().contains_key(&room2.source_room_id));
            assert!(connector.get_group_id_mapping().await.unwrap().contains_key(&room3.source_room_id));
        }

        #[rstest]
        #[tokio::test]
        async fn and_obtaining_mapping_for_one_user_fails_then_still_process_other_user(mut connector: Connector) {
            // given
            let user1 = MockSynapseUserBuilder::default().build();
            let user2 = MockSynapseUserBuilder::default().build();
            let user3 = MockSynapseUserBuilder::default().build();

            connector.synapse_api = SynapseApiMocker::new()
                .with_users(vec![user1.clone(), user2.clone(), user3.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_users()
                .can_get_source_user_id_for_matrix_user(&user1)
                .cannot_get_source_user_id_for_matrix_user(&user2)
                .can_get_source_user_id_for_matrix_user(&user3)
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert!(connector.get_user_id_mapping().await.unwrap().contains_key(&user1.source_user_id));
            assert!(!connector.get_user_id_mapping().await.unwrap().contains_key(&user2.source_user_id));
            assert!(connector.get_user_id_mapping().await.unwrap().contains_key(&user3.source_user_id));
        }

        #[rstest]
        #[tokio::test]
        async fn but_cannot_get_joined_rooms_of_syncer_then_return_err(mut connector: Connector) {
            // given
            connector.synapse_api = SynapseApiMocker::new().cannot_get_joined_rooms_of_syncer().into();

            // when & then
            assert!(connector.full_sync_incoming().await.is_ok());
            assert!(connector.generate_id_mappings().await.is_err());
        }

        #[rstest]
        #[tokio::test]
        async fn but_cannot_get_users_then_return_err(mut connector: Connector) {
            // given
            connector.synapse_api = SynapseApiMocker::new().can_get_joined_rooms_of_syncer().cannot_get_users().into();

            // when & then
            assert!(connector.full_sync_incoming().await.is_ok());
            assert!(connector.generate_id_mappings().await.is_err());
        }
    }

    mod manage_groups {
        use super::*;

        #[derive(Debug, PartialEq, Eq, Clone)]
        pub(super) struct Group {
            id: String,
            attributes: std::collections::HashMap<String, Vec<String>>,
        }

        impl Group {
            pub fn new(id: impl Into<String>, attributes: Option<std::collections::HashMap<String, Vec<String>>>) -> Self {
                Self {
                    id: id.into(),
                    attributes: attributes.unwrap_or_default(),
                }
            }
        }

        impl From<Group> for std::sync::Arc<dyn source::interface::Group + Send + Sync> {
            fn from(value: Group) -> Self {
                std::sync::Arc::new(value)
            }
        }

        #[async_trait::async_trait(?Send)]
        impl crate::source::interface::Group for Group {
            fn id(&self) -> &types::SharedResourceIdentifier {
                &self.id
            }

            fn name(&self) -> &str {
                &self.id
            }

            fn path(&self) -> &str {
                &self.id
            }

            fn attributes(&self) -> &std::collections::HashMap<String, Vec<String>> {
                &self.attributes
            }

            fn root_group(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn crate::source::interface::Group> {
                self
            }

            fn parent_group(&self) -> Option<std::sync::Arc<dyn crate::source::interface::Group>> {
                None
            }

            async fn sub_groups(self: std::sync::Arc<Self>) -> Result<Vec<std::sync::Arc<dyn crate::source::interface::Group>>, error::KidsError> {
                Ok(vec![])
            }
        }

        mod create {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn create_group_succeeds_without_attribute(mut connector: Connector) {
                // given
                let group = Group::new("group", None);
                let group_id = group.id.clone();
                connector.synapse_api = SynapseApiMocker::new()
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .cannot_create_room()
                    .into();

                // when
                let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;

                // then
                created.expect("Error creating or updating group");
                let all_groups = connector.all_groups().await.unwrap();
                assert!(!all_groups.contains(&group_id));
            }

            #[rstest]
            #[tokio::test]
            async fn create_group_succeeds_with_attribute(mut connector: Connector) {
                // given
                let group = Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.synapse_api = SynapseApiMocker::new()
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_create_room()
                    .can_associate_source_group_id_to_room()
                    .can_get_room_display_name_all_rooms()
                    .can_full_room_alias()
                    .can_get_room_canonical_alias_all_rooms()
                    .into();

                // when
                let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;

                // then
                created.expect("Error creating or updating group");
                let all_groups = connector.all_groups().await.unwrap();
                assert!(all_groups.contains(&group_id));
            }

            #[rstest]
            #[tokio::test]
            async fn create_group_is_idempotent(mut connector: Connector) {
                // given
                let group = Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                {
                    // 1. Add
                    connector.synapse_api = SynapseApiMocker::new()
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        .can_create_room()
                        .can_associate_source_group_id_to_room()
                        .can_get_room_display_name_all_rooms()
                        .can_full_room_alias()
                        .can_get_room_canonical_alias_all_rooms()
                        .into();

                    // when
                    let created = connector.create_or_update_group(std::sync::Arc::new(group.clone())).await;

                    // then
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                }
                {
                    // 2. Do nothing
                    connector.synapse_api = SynapseApiMocker::new()
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        // We disallow room creation here, it must use the existing one instead.
                        .cannot_create_room()
                        .can_get_room_display_name_all_rooms()
                        .can_full_room_alias()
                        .can_get_room_canonical_alias_all_rooms()
                        .into();

                    // when
                    let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;

                    // then
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                }
            }

            #[rstest]
            #[tokio::test]
            #[case("C:\\Windows")]
            #[tokio::test]
            #[case("/usr/bin")]
            async fn create_group_fails_with_invalid_group_name(mut connector: Connector, #[case] invalid_group_name: &'static str) {
                // given
                let group = Group::new(invalid_group_name, None);
                connector.synapse_api = SynapseApiMocker::new().can_get_joined_rooms_of_syncer().can_get_users().into();

                // when
                let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;

                // then
                let err: error::KidsError = created.expect_err("Creating or updating group unexpectedly succeeded");
                match err {
                    crate::error::KidsError::InternalError(ref msg)
                        if *msg == format!("Could not create room for group {invalid_group_name}: group name contains invalid character") => {}
                    ref err => panic!("Error creating or updating group: {err:?}."),
                };
            }
        }

        mod update {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn update_group_succeeds_updates_room_existence(mut connector: Connector) {
                // given
                let mut group = Group::new("group", None);
                let group_id = group.id.clone();
                connector.synapse_api = SynapseApiMocker::new()
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_create_room()
                    .can_associate_source_group_id_to_room()
                    .can_get_room_display_name_all_rooms()
                    .can_full_room_alias()
                    .can_get_room_canonical_alias_all_rooms()
                    .into();
                {
                    // 1. Without attribute, nothing happens.

                    // when
                    let created = connector.create_or_update_group(std::sync::Arc::new(group.clone())).await;

                    // then
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(!all_groups.contains(&group_id));
                }
                {
                    // 2. With attribute, the group is now included.
                    group
                        .attributes
                        .insert(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()]);

                    // when
                    let created = connector.create_or_update_group(std::sync::Arc::new(group.clone())).await;

                    // then
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                }
                {
                    // 3. Without attribute, the group gets removed.
                    group.attributes.clear();

                    // when
                    let created = connector.create_or_update_group(std::sync::Arc::new(group.clone())).await;

                    // then
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(!all_groups.contains(&group_id));
                }
            }

            #[rstest]
            #[tokio::test]
            #[case("Other name", "Other name")]
            #[tokio::test]
            #[case(DERIVE_DISPLAY_NAME_FROM_GROUP_NAME, "group Group Group group")]
            async fn update_group_changes_name(mut connector: Connector, #[case] attr: &'static str, #[case] expected_name: &'static str) {
                // given
                let mut group = Group::new(
                    // Complex pattern for `DERIVE_DISPLAY_NAME_FROM_GROUP_NAME` handling.
                    "group-Group_Group group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                let matrix_room_id = {
                    // 1. Add
                    connector.synapse_api = SynapseApiMocker::new()
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        .can_create_room()
                        .can_associate_source_group_id_to_room()
                        .can_get_room_display_name_all_rooms()
                        .can_full_room_alias()
                        .can_get_room_canonical_alias_all_rooms()
                        .into();

                    // when
                    let created = connector.create_or_update_group(std::sync::Arc::new(group.clone())).await;

                    // then
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                    connector.get_group_id_mapping().await.unwrap().get(&group_id).unwrap()
                }
                .to_owned();
                {
                    // 2. Update group name
                    let new_group_name = attr;
                    group.attributes.entry(connector.config.source_room_name_attr.clone()).and_modify(|entry| {
                        entry.clear();
                        entry.push(new_group_name.to_owned());
                    });
                    connector.synapse_api = SynapseApiMocker::new()
                        .with_rooms(vec![MockSynapseRoomBuilder::default()
                            .source_room_id(group_id.clone())
                            .matrix_room_id(matrix_room_id.clone())
                            .build()])
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        // We disallow room creation here, it must use the existing one instead.
                        .cannot_create_room()
                        .can_get_room_display_name_all_rooms()
                        .require_set_room_display_name(matrix_room_id.clone(), expected_name)
                        .can_full_room_alias()
                        .can_get_room_canonical_alias_all_rooms()
                        .require_set_room_canonical_alias(matrix_room_id.clone())
                        .require_create_room_alias(matrix_room_id)
                        .can_delete_room_alias_all_aliases()
                        .into();

                    // when
                    let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;

                    // then
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                }
            }
        }

        mod delete {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn delete_group_ignore_room_deletion(mut connector: Connector) {
                // given
                let group = Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.synapse_api = SynapseApiMocker::new()
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_create_room()
                    .can_associate_source_group_id_to_room()
                    .can_get_room_display_name_all_rooms()
                    .can_full_room_alias()
                    .can_get_room_canonical_alias_all_rooms()
                    .into();
                {
                    let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                }
                {
                    // when
                    let deleted = connector.delete_group(&group_id).await;

                    // then
                    deleted.expect("Error deleting group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(!all_groups.contains(&group_id));
                }
            }

            #[rstest]
            #[tokio::test]
            #[case(RoomDeletionStrategy::KickAll)]
            #[tokio::test]
            #[case(RoomDeletionStrategy::Evacuate)]
            #[tokio::test]
            #[case(RoomDeletionStrategy::Delete)]
            async fn delete_group_kickall_evacuate_delete_room_deletion(mut connector: Connector, #[case] deletion_strategy: RoomDeletionStrategy) {
                // given
                let group = Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.config.room_deletion_strategy = deletion_strategy;
                connector.synapse_api = SynapseApiMocker::new()
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_create_room()
                    .can_associate_source_group_id_to_room()
                    .can_get_room_display_name_all_rooms()
                    .can_full_room_alias()
                    .can_get_room_canonical_alias_all_rooms()
                    .into();
                let matrix_room_id = {
                    let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                    connector.get_group_id_mapping().await.unwrap().get(&group_id).unwrap()
                };
                connector.synapse_api = {
                    let mut mock_api = SynapseApiMocker::new().with_rooms(vec![MockSynapseRoomBuilder::default()
                        .source_room_id(group_id.clone())
                        .matrix_room_id(matrix_room_id.clone())
                        .build()]);
                    if matches!(deletion_strategy, RoomDeletionStrategy::KickAll | RoomDeletionStrategy::Evacuate) {
                        // Managing room members is necessary to kick users.
                        mock_api = mock_api.can_manage_room_members(
                            matrix_room_id,
                            "syncer-user",
                            ["user-1", "user-2"],
                            matches!(deletion_strategy, RoomDeletionStrategy::Evacuate),
                            None,
                        );
                        if matches!(deletion_strategy, RoomDeletionStrategy::Evacuate) {
                            mock_api = mock_api.can_get_room_canonical_alias_all_rooms().can_delete_room_alias_all_aliases();
                        }
                    } else {
                        mock_api = mock_api.require_delete_room(matrix_room_id.clone());
                    }
                    mock_api.into()
                };
                {
                    // when
                    let deleted = connector.delete_group(&group_id).await;

                    // then
                    deleted.expect("Error deleting group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(!all_groups.contains(&group_id));
                }
            }

            #[rstest]
            #[tokio::test]
            #[case(RoomDeletionStrategy::KickAll)]
            #[tokio::test]
            #[case(RoomDeletionStrategy::Evacuate)]
            async fn delete_group_kickall_evacuate_room_deletion_fails_without_kicking(
                mut connector: Connector,
                #[case] deletion_strategy: RoomDeletionStrategy,
            ) {
                // given
                let group = Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.config.room_deletion_strategy = deletion_strategy;
                connector.synapse_api = SynapseApiMocker::new()
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_create_room()
                    .can_associate_source_group_id_to_room()
                    .can_get_room_display_name_all_rooms()
                    .can_full_room_alias()
                    .can_get_room_canonical_alias_all_rooms()
                    .into();
                let matrix_room_id = {
                    let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;
                    created.expect("Error creating or updating group");
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                    connector.get_group_id_mapping().await.unwrap().get(&group_id).unwrap()
                }
                .to_owned();
                connector.synapse_api = {
                    let mut mock_api = SynapseApiMocker::new().with_rooms(vec![MockSynapseRoomBuilder::default()
                        .source_room_id(group_id.clone())
                        .matrix_room_id(matrix_room_id.clone())
                        .build()]);
                    // Managing room members is necessary to kick users.
                    // We disallow the syncer to leave the room as we will fail kicking all users.
                    // In that case, the syncer must not leave the room.
                    mock_api = mock_api.can_manage_room_members(matrix_room_id.clone(), "syncer-user", ["user-1", "user-2"], false, Some("user-1"));
                    mock_api.into()
                };
                {
                    // when
                    let deleted = connector.delete_group(&group_id).await;

                    // then
                    let err: error::KidsError = deleted.expect_err("Deleting group unexpectedly succeeded");
                    match err {
                        crate::error::KidsError::InternalError(ref msg) if *msg == format!("Could not kick all members from room {matrix_room_id}") => {}
                        ref err => panic!("Error deleting group: {err:?}."),
                    };
                    let all_groups = connector.all_groups().await.unwrap();
                    assert!(all_groups.contains(&group_id));
                }
            }
        }
    }

    mod manage_users {
        use super::*;

        #[derive(Debug, PartialEq, Eq, Clone)]
        struct User {
            id: String,
            username: Option<String>,
            first_name: Option<String>,
            last_name: Option<String>,
            email: Option<String>,
            enabled: bool,
            attributes: std::collections::HashMap<String, Vec<String>>,
            groups: Vec<super::manage_groups::Group>,
        }

        impl User {
            #[expect(clippy::too_many_arguments)]
            pub fn new(
                id: impl Into<String>,
                username: Option<String>,
                first_name: Option<String>,
                last_name: Option<String>,
                email: Option<String>,
                enabled: bool,
                attributes: Option<std::collections::HashMap<String, Vec<String>>>,
                groups: Option<Vec<super::manage_groups::Group>>,
            ) -> Self {
                Self {
                    id: id.into(),
                    username,
                    first_name,
                    last_name,
                    email,
                    enabled,
                    attributes: attributes.unwrap_or_default(),
                    groups: groups.unwrap_or_default(),
                }
            }
        }

        #[async_trait::async_trait]
        impl crate::source::interface::User for User {
            fn id(&self) -> &types::SharedResourceIdentifier {
                &self.id
            }
            fn enabled(&self) -> bool {
                self.enabled
            }
            fn username(&self) -> Option<&str> {
                self.username.as_deref()
            }
            fn first_name(&self) -> Option<&str> {
                self.first_name.as_deref()
            }
            fn last_name(&self) -> Option<&str> {
                self.last_name.as_deref()
            }
            fn email(&self) -> Option<&str> {
                self.email.as_ref().map(|s| s.as_ref())
            }
            fn attributes(&self) -> &collections::HashMap<String, Vec<String>> {
                &self.attributes
            }

            async fn groups(
                &self,
                _include_transitive_groups: bool,
            ) -> Result<Vec<std::sync::Arc<dyn crate::source::interface::Group + Send + Sync>>, error::KidsError> {
                Ok(self.groups.clone().into_iter().map(Into::into).collect())
            }
        }

        /// Note that [`create_or_update_user`](Connector::create_or_update_user) can never create a new user account.
        /// We require that the user handles the first login manually, e.g. to setup the recovery key.
        mod create {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn create_user_succeeds_noop_without_being_present(mut connector: Connector) {
                // given
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let user = User::new("user", None, None, None, None, true, None, Some(vec![group.clone()]));
                let user_id = user.id.clone();
                connector.synapse_api = SynapseApiMocker::new()
                    .with_rooms(vec![MockSynapseRoomBuilder::default().source_room_id(group.id()).build()])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    .can_get_room_associated_source_group_id_v1()
                    .can_associate_source_group_id_to_room()
                    .can_get_all_rooms_associated_source_group_id()
                    .into();

                // when
                let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                // Users need to login themselves to be present in Synapse.
                assert!(!all_users.contains(&user_id));
            }

            #[rstest]
            #[tokio::test]
            async fn not_create_user_succeeds_adding_user(mut connector: Connector) {
                // given
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = User::new("user", None, None, None, None, true, None, Some(vec![group]));
                let user_id = user.id;
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector.synapse_api = SynapseApiMocker::new()
                    .with_rooms(vec![synapse_room])
                    .with_users(vec![synapse_user])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    .can_get_room_associated_source_group_id_v1()
                    .can_associate_source_group_id_to_room()
                    .can_get_all_rooms_associated_source_group_id()
                    .into();

                // when
                // nothing

                // then
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
            }
        }

        mod update {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn update_user_updates_name_email(mut connector: Connector) {
                // given
                let current_first_name = "First";
                let current_email = "my-email@example.com";
                let mut user = User::new(
                    "user",
                    None,
                    Some(current_first_name.to_owned()),
                    Some("Lastname".to_owned()),
                    Some(current_email.to_owned()),
                    true,
                    None,
                    None,
                );
                let new_first_name = "New First";
                let new_display_name = "New First Lastname";
                let new_email = "my-new-email@example.com";
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector.synapse_api = SynapseApiMocker::new()
                    .with_rooms(vec![])
                    .with_users(vec![synapse_user.clone()])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    // The user is no member of any (managed) room.
                    .can_get_joined_rooms_of_user(&synapse_user, vec![])
                    .can_get_user_display_name(
                        &synapse_user,
                        Some({
                            use crate::source::interface::User;
                            user.display_name().unwrap()
                        }),
                    )
                    .can_get_user_three_pids(&synapse_user, Some(current_email.to_owned()))
                    .require_set_user_display_name(&synapse_user, new_display_name)
                    .require_set_user_three_pids(&synapse_user, new_email)
                    .into();
                let created = connector.create_or_update_user(std::sync::Arc::new(user.clone())).await;
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));

                // when
                user.first_name = Some(new_first_name.to_owned());
                user.email = Some(new_email.to_owned());
                let updated = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                updated.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
            }

            #[rstest]
            #[tokio::test]
            async fn update_user_adds_room(mut connector: Connector) {
                // given
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = User::new("user", None, None, None, None, true, None, Some(vec![group]));
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector.synapse_api = SynapseApiMocker::new()
                    .with_rooms(vec![synapse_room.clone()])
                    .with_users(vec![synapse_user.clone()])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    .can_get_room_associated_source_group_id_v1()
                    .can_associate_source_group_id_to_room()
                    .can_get_all_rooms_associated_source_group_id()
                    .can_get_user_display_name(&synapse_user, None)
                    .can_get_user_three_pids(&synapse_user, None)
                    // Assume the user is not yet member of any (managed) room.
                    .can_get_joined_rooms_of_user(&synapse_user, vec![])
                    // This is the core assertion here: The user gets added to the room.
                    .require_join_user_to_room(&synapse_user, &synapse_room)
                    .into();

                // when
                let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
            }

            #[rstest]
            #[tokio::test]
            async fn update_user_leaves_room(mut connector: Connector) {
                // given
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = User::new("user", None, None, None, None, true, None, None);
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector.synapse_api = SynapseApiMocker::new()
                    .with_rooms(vec![synapse_room.clone()])
                    .with_users(vec![synapse_user.clone()])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    .can_get_room_associated_source_group_id_v1()
                    .can_associate_source_group_id_to_room()
                    .can_get_all_rooms_associated_source_group_id()
                    .can_get_user_display_name(&synapse_user, None)
                    .can_get_user_three_pids(&synapse_user, None)
                    // Assume the user is still member of the managed room.
                    .can_get_joined_rooms_of_user(&synapse_user, vec![&synapse_room])
                    // This is the core assertion here: The user gets kicked from the room.
                    .require_kick_user_from_room(&synapse_user, &synapse_room)
                    .into();

                // when
                let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
            }

            #[rstest]
            #[tokio::test]
            async fn update_user_locking(mut connector: Connector) {
                // given
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let mut user = User::new("user", None, None, None, None, true, None, Some(vec![group]));
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                let get_api_mocker = |joined_room: Vec<&crate::target::synapse::test_mocks::MockSynapseRoom>| {
                    SynapseApiMocker::new()
                        .with_rooms(vec![synapse_room.clone()])
                        .with_users(vec![synapse_user.clone()])
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        .can_get_source_user_id_for_all_matrix_users()
                        .can_get_room_associated_source_group_id_v1()
                        .can_associate_source_group_id_to_room()
                        .can_get_all_rooms_associated_source_group_id()
                        .can_get_user_display_name(&synapse_user, None)
                        .can_get_user_three_pids(&synapse_user, None)
                        .can_get_joined_rooms_of_user(&synapse_user, joined_room)
                };
                {
                    // 1. Create as unlocked.
                    // when
                    connector.synapse_api = get_api_mocker(vec![&synapse_room]).into();

                    // then
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.get_user_id_mapping().await.unwrap().get(&user_id).unwrap();
                    assert!(!user_via_connector.locked);
                }
                {
                    // 2. Update to locked.
                    user.enabled = false;
                    connector.synapse_api = get_api_mocker(vec![&synapse_room])
                        .require_lock_user(&synapse_user)
                        .require_kick_user_from_room(&synapse_user, &synapse_room)
                        .into();

                    // when
                    let created = connector.create_or_update_user(std::sync::Arc::new(user.clone())).await;

                    // then
                    created.expect("Error creating or updating user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.get_user_id_mapping().await.unwrap().get(&user_id).unwrap();
                    assert!(user_via_connector.locked);
                }
                {
                    // 3. Update to unlocked.
                    user.enabled = true;
                    connector.synapse_api = get_api_mocker(vec![])
                        .require_unlock_user(&synapse_user)
                        .require_join_user_to_room(&synapse_user, &synapse_room)
                        .into();

                    // when
                    let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                    // then
                    created.expect("Error creating or updating user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.get_user_id_mapping().await.unwrap().get(&user_id).unwrap();
                    assert!(!user_via_connector.locked);
                }
            }
        }

        mod delete {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn delete_user_deactivates_it(mut connector: Connector) {
                let group = super::manage_groups::Group::new(
                    // given
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = User::new("user", None, None, None, None, true, None, Some(vec![group]));
                let user_id = user.id;
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                {
                    // 1. Create user.
                    connector.synapse_api = SynapseApiMocker::new()
                        .with_rooms(vec![synapse_room])
                        .with_users(vec![synapse_user.clone()])
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        .can_get_source_user_id_for_all_matrix_users()
                        .can_get_room_associated_source_group_id_v1()
                        .can_associate_source_group_id_to_room()
                        .can_get_all_rooms_associated_source_group_id()
                        .into();
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                }
                {
                    // 2. Delete user.
                    connector.synapse_api = SynapseApiMocker::new().require_deactivate_user(&synapse_user).into();

                    // when
                    let deleted = connector.delete_user(&user_id).await;

                    // then
                    deleted.expect("Error deleting user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(!all_users.contains(&user_id));
                }
            }
        }
    }
}
