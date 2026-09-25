use std::collections;

use kids_lib::error::KidsError;

use crate::target::external;

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
    pub room_deletion_strategy: crate::target::RoomDeletionStrategy,
    /// Only users who have set this role will be handled by the syncer.
    /// When this is not present, all users will be added to Matrix.
    pub required_role_name: Option<String>,
    /// Users with this role set will be made admin and all others will not be admin.
    pub admin_role_name: String,
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
    synapse_interactor: crate::target::SynapseInteractor,
    mappings: crate::target::IdMapping,
}

impl Connector {
    async fn desired_state_from_source_user(
        source_user: &(dyn kids_lib::interface::source::User + Send + Sync),
        admin_role_name: &String,
        enforce_lock: bool,
        groups: &crate::target::GroupMapping,
    ) -> Result<crate::target::types::UserState, KidsError> {
        let display_name = source_user.display_name();
        let emails = source_user.email().map(|email| vec![email.to_owned()]).unwrap_or_default();
        let locked = !source_user.enabled() || enforce_lock;
        let is_admin = source_user.client_roles().await?.contains(admin_role_name);
        let rooms = if locked {
            // In case the user is locked, we want to kick them from all rooms.
            vec![]
        } else {
            Self::desired_user_rooms(groups, source_user).await?
        };
        Ok(crate::target::types::UserState {
            display_name,
            emails,
            locked,
            is_admin,
            rooms,
        })
    }

    async fn ensure_user_state(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
        desired_state: crate::target::types::UserState,
    ) -> Result<(), KidsError> {
        tracing::trace!(existing_user = ?matrix_user, ?desired_state, "Ensuring user state");
        Self::ensure_user_display_name(synapse_interactor, matrix_user, desired_state.display_name).await?;
        Self::ensure_user_email(synapse_interactor, matrix_user, desired_state.emails).await?;
        Self::ensure_user_locked_state_in_sync(synapse_interactor, matrix_user, desired_state.locked).await?;
        Self::ensure_user_is_admin(synapse_interactor, matrix_user, desired_state.is_admin).await?;
        Self::ensure_user_rooms(synapse_interactor, matrix_user, desired_state.rooms).await?;
        Ok(())
    }

    async fn ensure_user_is_admin(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
        should_be_admin: bool,
    ) -> Result<(), kids_lib::error::KidsError> {
        let is_admin = matrix_user.state.is_admin;
        if should_be_admin == is_admin {
            tracing::trace!(
                matrix_user_id = matrix_user.matrix_user_id.display(),
                source_user_id = matrix_user.source_user_id,
                is_admin,
                "Keeping existing admin status"
            );
            return Ok(());
        }
        tracing::info!(
            matrix_user_id = matrix_user.matrix_user_id.display(),
            is_admin,
            should_be_admin,
            "Changing admin status of user"
        );
        synapse_interactor
            .synapse_api()
            .set_admin_status(&matrix_user.mas_user_id, should_be_admin)
            .await?;
        matrix_user.state.is_admin = should_be_admin;
        Ok(())
    }

    async fn ensure_user_locked_state_in_sync(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
        should_be_locked: bool,
    ) -> Result<(), kids_lib::error::KidsError> {
        match should_be_locked {
            // Note that we explicitly want to lock users here, NOT deactivate them.
            // Deactivating users appears to delete all keys of that user, so even when a
            // user is reactivated, they cannot log in with the same identity and lose
            // all of their direct message rooms.
            // With locking, this works properly and unlocked users will encounter the same
            // state they left off with before being locked.
            true => Self::ensure_user_locked(synapse_interactor, matrix_user).await,
            false => Self::ensure_user_unlocked(synapse_interactor, matrix_user).await,
        }
    }

    async fn ensure_user_locked(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
    ) -> Result<(), kids_lib::error::KidsError> {
        let matrix_user_id = &matrix_user.matrix_user_id;
        if !matrix_user.state.locked {
            // Note that we explicitly want to lock users here, NOT deactivate them.
            // Deactivating users appears to delete all keys of that user, so even when a
            // user is reactivated, they cannot log in with the same identity and lose
            // all of their direct message rooms.
            // With locking, this works properly and unlocked users will encounter the same
            // state they left off with before being locked.
            match synapse_interactor.synapse_api().lock_user(&matrix_user.mas_user_id).await {
                Ok(()) => {
                    // Write lock state to user object.
                    matrix_user.state.locked = true;
                    tracing::info!(matrix_user_id = matrix_user_id.display(), "Locked user");
                }
                Err(e) => {
                    tracing::error!(?e, matrix_user_id = matrix_user_id.display(), "Could not lock user");
                    return Err(e);
                }
            };
        }
        Ok(())
    }

    async fn ensure_user_unlocked(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
    ) -> Result<(), kids_lib::error::KidsError> {
        let matrix_user_id = &matrix_user.matrix_user_id;
        if matrix_user.state.locked {
            match synapse_interactor.synapse_api().unlock_user(&matrix_user.mas_user_id).await {
                Ok(()) => {
                    // Write lock state to user object.
                    matrix_user.state.locked = false;
                    tracing::info!(matrix_user_id = matrix_user_id.display(), "Unlocked user");
                }
                Err(e) => {
                    tracing::error!(?e, matrix_user_id = matrix_user_id.display(), "Could not unlock user");
                    return Err(e);
                }
            };
        }
        if matrix_user.deactivated {
            match synapse_interactor.synapse_api().reactivate_user(&matrix_user.mas_user_id).await {
                Ok(()) => {
                    // Write deactivation state to user object.
                    matrix_user.deactivated = false;
                    tracing::info!(matrix_user_id = matrix_user_id.display(), "Reactivated user");
                }
                Err(e) => {
                    tracing::error!(?e, matrix_user_id = matrix_user_id.display(), "Could not reactivate user");
                    return Err(e);
                }
            };
        }
        Ok(())
    }

    async fn ensure_user_display_name(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
        desired_display_name: Option<String>,
    ) -> Result<(), KidsError> {
        if matrix_user.state.display_name != desired_display_name {
            let matrix_user_id = &matrix_user.matrix_user_id;
            tracing::debug!(
                matrix_user_id = tracing::field::display(matrix_user_id),
                source_user_id = matrix_user.source_user_id,
                old_display_name = matrix_user.state.display_name,
                new_display_name = desired_display_name,
                "Updating user's display name."
            );
            if let Some(desired_name) = desired_display_name {
                synapse_interactor
                    .synapse_api()
                    .set_user_display_name(matrix_user_id, desired_name.as_str())
                    .await?;
                matrix_user.state.display_name = Some(desired_name);
            } else {
                const ERROR_CONTEXT: &str = "Creating or updating user";
                const ERROR_MSG: &str = "Requested to unset the display name of a user. This is impossible in Matrix.";
                tracing::error!(source_user_id = matrix_user.source_user_id, "{ERROR_CONTEXT}: {ERROR_MSG}");
                return Err(kids_lib::error::KidsError::RequestFailed(
                    ERROR_CONTEXT.to_owned(),
                    anyhow::anyhow!("{ERROR_MSG}"),
                ));
            }
        }
        Ok(())
    }

    async fn ensure_user_email(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
        desired_emails: Vec<String>,
    ) -> Result<(), KidsError> {
        if matrix_user.state.emails != desired_emails {
            tracing::debug!(
                matrix_user_id = tracing::field::display(&matrix_user.matrix_user_id),
                source_user_id = matrix_user.source_user_id,
                old_emails = ?matrix_user.state.emails,
                new_emails = ?desired_emails,
                "Updating user's emails."
            );
            synapse_interactor
                .synapse_api()
                .set_user_emails(&matrix_user.mas_user_id, desired_emails.as_slice())
                .await?;
            matrix_user.state.emails = desired_emails;
        }
        Ok(())
    }

    async fn desired_user_rooms(
        groups: &crate::target::GroupMapping,
        source_user: &(dyn kids_lib::interface::source::User + Send + Sync),
    ) -> Result<Vec<String>, KidsError> {
        let desired_user_groups = source_user
            .groups(true)
            .await
            .map_err(|e| e.with_context(&format!("Could not get source groups associated with source user {}", source_user.id())))?;

        Ok(desired_user_groups
            .iter()
            .filter_map(|group| {
                // We only want to add the user to groups that have a corresponding matrix room.
                // Note: On the initial run of the syncer, i.e. before any rooms are created,
                // we gradually add the users to the rooms, as we iterate over all users after creating a room.
                // On all further runs, the mapping contains the existing rooms and thus all possible rooms
                // a user could desire.
                groups.get_group_opt(group.id()).map(ToOwned::to_owned)
            })
            .collect())
    }

    async fn ensure_user_rooms(
        synapse_interactor: &crate::target::SynapseInteractor,
        matrix_user: &mut crate::target::types::User,
        desired_user_rooms: Vec<String>,
    ) -> Result<(), KidsError> {
        let matrix_user_id = &matrix_user.matrix_user_id;
        let current_user_rooms = matrix_user.state.rooms.as_slice();

        let mut newly_joined_rooms = vec![];
        let mut newly_kicked_rooms = vec![];

        // Add user to all desired groups that they are not already joined to.
        for matrix_room_id in &desired_user_rooms {
            if !current_user_rooms.contains(matrix_room_id) {
                match synapse_interactor.synapse_api().join_user_to_room(matrix_room_id, matrix_user_id).await {
                    Ok(()) => {
                        tracing::info!(matrix_room_id, matrix_user_id = matrix_user_id.display(), "User joined matrix room");
                        newly_joined_rooms.push(matrix_room_id.to_owned());
                    }
                    Err(e) => {
                        tracing::error!(
                            ?e,
                            matrix_room_id,
                            matrix_user_id = matrix_user_id.display(),
                            "Could not join user to matrix room"
                        );
                        return Err(e);
                    }
                }
            } else {
                tracing::trace!(matrix_room_id, matrix_user_id = matrix_user_id.display(), "User has already joined matrix room");
            }
        }

        // We can only remove a user from rooms in which we are members as well.
        // This is not an issue for adding as all rooms desired are derived from source groups
        // where we have created the associated room ourselves and the syncer user is a member.
        // However, in manually created rooms we cannot remove the user.
        // We also should not attempt to do it as we would not re-add the user once they are unlocked.
        let managed_rooms = synapse_interactor.synapse_api().get_joined_rooms_of_syncer().await?;

        // Remove user from all joined groups that are no longer desired.
        for matrix_room_id in current_user_rooms {
            if !managed_rooms.joined_rooms.contains(matrix_room_id) {
                tracing::debug!(matrix_room_id, matrix_user_id = matrix_user_id.display(), "User stays in unmanaged room");
                continue;
            }
            if desired_user_rooms.contains(matrix_room_id) {
                tracing::trace!(
                    matrix_room_id,
                    matrix_user_id = matrix_user_id.display(),
                    "User stays in managed matrix room as it is still desired"
                );
                continue;
            }
            match synapse_interactor.synapse_api().kick_user_from_room(matrix_room_id, matrix_user_id).await {
                Ok(()) => {
                    tracing::info!(matrix_room_id, matrix_user_id = matrix_user_id.display(), "User kicked from matrix room");
                    newly_kicked_rooms.push(matrix_room_id.to_owned());
                }
                Err(e) => {
                    tracing::error!(
                        ?e,
                        matrix_room_id,
                        matrix_user_id = matrix_user_id.display(),
                        "Could not kick user from matrix room"
                    );
                    return Err(e);
                }
            };
        }

        matrix_user.state.rooms.retain(|room| !newly_kicked_rooms.contains(room));
        matrix_user.state.rooms.append(&mut newly_joined_rooms);
        Ok(())
    }

    /// Returns `true` when the user has the required role in Source
    /// or when the the config option `required_role_name` is unset.
    async fn source_user_has_required_role(&self, source_user: &(dyn kids_lib::interface::source::User + Send + Sync)) -> Result<bool, KidsError> {
        if let Some(required_role_name) = &self.config.required_role_name {
            let roles = source_user.client_roles().await?;
            let required_role_present = roles.contains(required_role_name);
            return Ok(required_role_present);
        }
        // If no required role is set, pass this test
        Ok(true)
    }
}

#[async_trait::async_trait]
impl kids_lib::interface::target::Target for Connector {
    type Config = SynapseConfig;

    async fn new(config: Self::Config) -> Result<Self, KidsError> {
        let synapse_api = external::SynapseClient::new(config.synapse_api.clone())
            .await
            .map_err(|e| e.with_context("Failed to create Synapse API client"))?;
        let synapse_interactor = crate::target::SynapseInteractor::new(synapse_api);
        let mappings = crate::target::IdMapping::generate(&synapse_interactor).await?;
        Ok(Connector {
            config,
            synapse_interactor,
            mappings,
        })
    }

    fn info(&self) -> String {
        "Synapse Connector!".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), KidsError> {
        tracing::info!(
            "To prepare for full sync, re-building mapping between source group IDs and matrix room IDs, as well as source user IDs and matrix user IDs"
        );
        self.mappings = crate::target::IdMapping::generate(&self.synapse_interactor).await?;

        Ok(())
    }

    /// Return the identifiers of all [Source Groups](kids_lib::interface::source::Group) known to Synapse.
    /// These are exactly the ones we managed to obtain a mapping to a Matrix room for earlier.
    /// There might be additional rooms in Synapse not mapped to a Source group, which will not be considered in the result of this method.
    async fn all_groups(&mut self) -> Result<collections::HashSet<kids_lib::types::SharedResourceIdentifier>, KidsError> {
        Ok(self.mappings.group_id_mapping.get_group_id_mapping().keys().cloned().collect())
    }

    async fn all_users(&mut self) -> Result<collections::HashSet<kids_lib::types::SharedResourceIdentifier>, KidsError> {
        Ok(self.mappings.user_id_mapping.get_user_id_mapping().keys().cloned().collect())
    }

    async fn delete_group(&mut self, source_group_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), KidsError> {
        let matrix_room_id = match self.mappings.group_id_mapping.get_group_opt(source_group_id) {
            Some(matrix_room) => matrix_room,
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

        self.synapse_interactor.delete_room(matrix_room_id, self.config.room_deletion_strategy).await?;

        self.mappings.group_id_mapping.get_group_id_mapping_mut().remove(source_group_id);

        Ok(())
    }

    async fn delete_user(&mut self, user_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), KidsError> {
        if let Some(syncer_source_user_id) = self.mappings.user_id_mapping.get_syncer_source_user_id()
            && syncer_source_user_id == user_id
        {
            tracing::trace!(source_user_id = user_id, "Ignoring Syncer user.");
            return Ok(());
        }

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

        // However, as deactivating is a destructive action that is not properly reversible,
        // we decided against using it and instead only lock the user accounts,
        // just like when they are disabled in the source.
        // This has the same effect to the user, i.e., they cannot login and cannot perform any action.

        let matrix_user = match self.mappings.user_id_mapping.get_user_opt_mut(user_id) {
            Some(matrix_user) => matrix_user,
            None => {
                // This should not happen, as the controller should only attempt to delete users that
                // we told it exists in Matrix before via the `self.all_users` method.
                tracing::warn!(source_user_id = user_id, "Cannot lock source user, because it is not known to Matrix");
                return Ok(());
            }
        };

        let desired_state = crate::target::types::UserState {
            display_name: matrix_user.state.display_name.clone(),
            // Unset the email as otherwise, in case the person would get re-added as a new source user,
            // we would try to add the same email for a different matrix (MAS) user which does not work.
            emails: vec![],
            locked: true,
            is_admin: false,
            rooms: vec![],
        };

        Self::ensure_user_state(&self.synapse_interactor, matrix_user, desired_state).await?;

        Ok(())
    }

    async fn create_or_update_group(&mut self, source_group: std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>) -> Result<(), KidsError> {
        tracing::debug!(source_group_id = source_group.id(), "Create or update group");

        // The target does only care about groups with the domain-specific attribute.
        let room_name_attr = match self.get_room_name_attr(source_group.as_ref()) {
            Some(room_name_attr) => room_name_attr,
            None => {
                // if !source_group.attributes().contains_key(&self.config.source_room_name_attr) {
                match self.mappings.group_id_mapping.get_group_opt(source_group.id()) {
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
        };

        let matrix_room_id = &self.get_or_create_room(&source_group).await?.to_owned();
        self.update_display_name(matrix_room_id, room_name_attr, source_group.name()).await;
        self.update_canonical_alias(matrix_room_id, &source_group).await;

        // Update all contained users (transitively).
        // Otherwise, a room newly containing the room name attr would not be populated
        // as users are only added to rooms when they are themselves updated.
        for source_user in source_group.users(true).await? {
            self.create_or_update_user(source_user).await?;
        }

        Ok(())
    }

    async fn create_or_update_user(&mut self, source_user: std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>) -> Result<(), KidsError> {
        tracing::debug!(source_user_id = source_user.id(), "Create or update user");
        if let Some(syncer_source_user_id) = self.mappings.user_id_mapping.get_syncer_source_user_id()
            && syncer_source_user_id == source_user.id()
        {
            tracing::trace!(source_user_id = source_user.id(), "Ignoring Syncer user.");
            return Ok(());
        }

        let matrix_user_known = self.mappings.user_id_mapping.has_user(source_user.id());
        let source_user_has_required_role = self.source_user_has_required_role(source_user.as_ref()).await?;
        if !matrix_user_known && !source_user_has_required_role {
            // Exit early and do not create the user as the user is missing the required role.
            // If user already exists, it will be locked later
            tracing::trace!(source_user_id = source_user.id(), "Ignoring user that has no access to Matrix");
            return Ok(());
        }

        let matrix_user = Self::get_or_create_user(&self.synapse_interactor, &mut self.mappings.user_id_mapping, source_user.as_ref()).await?;

        let desired_state = Self::desired_state_from_source_user(
            source_user.as_ref(),
            &self.config.admin_role_name,
            !source_user_has_required_role,
            &self.mappings.group_id_mapping,
        )
        .await?;
        Self::ensure_user_state(&self.synapse_interactor, matrix_user, desired_state).await?;

        Ok(())
    }
}

impl Connector {
    fn generate_matrix_user_id(
        synapse_interactor: &crate::target::SynapseInteractor,
        source_user: &(dyn kids_lib::interface::source::User + Send + Sync),
    ) -> Result<crate::target::types::MatrixUserId, KidsError> {
        match source_user.username() {
            Some(username) => Ok(synapse_interactor.generate_matrix_user_id(username)),
            None => {
                const ERROR_CONTEXT: &str = "Generating matrix user id";
                const ERROR_MSG: &str = "The matrix user id depends on the source username to be set but it was not.";
                tracing::error!(user_id = source_user.id(), "{ERROR_CONTEXT}: {ERROR_MSG}");
                Err(kids_lib::error::KidsError::RequestFailed(
                    ERROR_CONTEXT.to_owned(),
                    anyhow::anyhow!("{ERROR_MSG}"),
                ))
            }
        }
    }

    async fn get_or_create_user<'a>(
        synapse_interactor: &'a crate::target::SynapseInteractor,
        user_mapping: &'a mut crate::target::UserMapping,
        source_user: &'a (dyn kids_lib::interface::source::User + Send + Sync),
    ) -> Result<&'a mut crate::target::types::User, KidsError> {
        // Unfortunately, `match` did not work here for lifetime reasons.
        if !user_mapping.has_user(source_user.id()) {
            let matrix_user_id = Self::generate_matrix_user_id(synapse_interactor, source_user)?;
            tracing::info!(
                source_user_id = source_user.id(),
                source_user_name = source_user.username(),
                matrix_user_id = matrix_user_id.display(),
                "Creating user"
            );
            let matrix_user = synapse_interactor.create_user(&matrix_user_id, source_user.id()).await?;
            user_mapping.get_user_id_mapping_mut().insert(source_user.id().clone(), matrix_user);
            tracing::info!(
                source_id = source_user.id(),
                username = source_user.username(),
                matrix_user_id = user_mapping.get_user(source_user.id()).matrix_user_id.display(),
                "User created"
            );
        }
        Ok(user_mapping.get_user_opt_mut(source_user.id()).expect("We have just added that user."))
    }

    async fn get_or_create_room(
        &mut self,
        source_group: &std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>,
    ) -> Result<&mut String, KidsError> {
        // Unfortunately, `match` did not work here for lifetime reasons.
        if !self.mappings.group_id_mapping.has_group(source_group.id()) {
            tracing::info!(
                source_group_id = source_group.id(),
                source_group_name = source_group.name(),
                "Creating room for group"
            );
            let room_creation_response = self
                .synapse_interactor
                .synapse_api()
                .create_room(source_group.name(), source_group.path())
                .await
                .map_err(|e| e.with_context("Could not create room"))?;
            let matrix_room_id = room_creation_response.room_id;
            self.synapse_interactor
                .synapse_api()
                .associate_source_group_id_to_room(&matrix_room_id, source_group.id())
                .await
                .map_err(|e| e.with_context(&format!("Could not associate source group id {} to room {}", source_group.id(), matrix_room_id)))?;
            self.mappings
                .group_id_mapping
                .get_group_id_mapping_mut()
                .insert(source_group.id().to_owned(), matrix_room_id);
            tracing::info!(
                source_id = source_group.id(),
                group_name = source_group.name(),
                matrix_room_id = self.mappings.group_id_mapping.get_group(source_group.id()),
                "Room created"
            );
        }
        Ok(self
            .mappings
            .group_id_mapping
            .get_group_opt_mut(source_group.id())
            .expect("We have just added that group."))
    }

    fn get_room_name_attr(&self, source_group: &(dyn kids_lib::interface::source::Group + Send + Sync)) -> Option<String> {
        let attribute_name = self.config.source_room_name_attr.as_str();
        match source_group.attributes().get(attribute_name) {
            Some(attribute) => match attribute.len() {
                0 => {
                    tracing::warn!(
                        source_group_id = source_group.id(),
                        attribute_name,
                        "Did find the configured attribute but it did not contain any data"
                    );
                    None
                }
                1 => Some(attribute.first().expect("We have just matched on the length").clone()),
                2.. => {
                    tracing::warn!(
                        source_group_id = source_group.id(),
                        attribute_name,
                        "Encountered multiple values for the configured attribute. Will only consider the first one for the room name"
                    );
                    Some(attribute.first().expect("We have just matched on the length").clone())
                }
            },
            None => None,
        }
    }

    fn get_room_desired_display_name(&self, room_name_attr: String, source_group_name: &str) -> String {
        if room_name_attr == DERIVE_DISPLAY_NAME_FROM_GROUP_NAME {
            source_group_name.replace("_", " ").replace("-", " ")
        } else {
            room_name_attr
        }
    }

    /// Update the display name of the room to match the one specified by the source group.
    ///
    /// This method expects the self.config.source_room_name_attr to be set on the source group.
    /// It should only be called on groups were that's the case (it will panic otherwise).
    async fn update_display_name(&self, matrix_room_id: &str, room_name_attr: String, source_group_name: &str) {
        let desired_name = self.get_room_desired_display_name(room_name_attr, source_group_name);
        self.synapse_interactor.ensure_group_display_name(matrix_room_id, desired_name).await;
    }

    async fn update_canonical_alias(&self, matrix_room_id: &str, source_group: &std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>) {
        let full_room_alias = self.synapse_interactor.synapse_api().full_room_alias(source_group.path());
        self.synapse_interactor.ensure_group_canonical_alias(matrix_room_id, full_room_alias).await;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::target::test_mocks::{MockSynapseRoomBuilder, MockSynapseUserBuilder, SynapseApiMocker};
    use kids_lib::interface::source::Group;
    use kids_lib::interface::target::Target;
    use rstest::*;

    const REQUIRED_ROLE: &str = "feature:authenticate";
    const ADMIN_ROLE: &str = "feature:admin";
    const SYNCER_USER_ID: &str = "@syncer-user:example.com";

    #[fixture]
    pub fn connector() -> Connector {
        Connector {
            config: SynapseConfig {
                synapse_api: external::SynapseApiConfig {
                    matrix_homeserver_url: "".to_string(),
                    matrix_mas_url: "".to_string(),
                    matrix_source_oidc_provider_ulid: "".to_string(),
                    matrix_syncer_user_id: "@syncer:example.com".into(),
                    matrix_namespace: "".to_string(),
                    insecure_disable_tls_verification: true,
                    api_access: external::ApiAccessConfig {
                        mas_client_id: "".to_owned(),
                        mas_client_secret: "".to_owned(),
                        token_validity_seconds: 120,
                    },
                },
                room_deletion_strategy: crate::target::RoomDeletionStrategy::Ignore,
                source_room_name_attr: "test".to_string(),
                required_role_name: Some(REQUIRED_ROLE.to_owned()),
                admin_role_name: ADMIN_ROLE.to_owned(),
            },
            synapse_interactor: SynapseApiMocker::new(SYNCER_USER_ID).into(),
            mappings: crate::target::IdMapping::empty(),
        }
    }

    impl Connector {
        /// Replaces the API mock **and** performs a full sync.
        ///
        /// This allows you to refer to new users not added via the [Connector] itself.
        async fn replace_api_mock(&mut self, synapse_api: crate::target::test_mocks::SynapseApiMocker) {
            self.synapse_interactor = synapse_api.into();
            self.full_sync_incoming().await.expect("full_sync_incoming should not fail");
        }
    }

    #[rstest]
    fn info_works(connector: Connector) {
        assert_eq!(connector.info(), "Synapse Connector!")
    }

    mod when_full_sync_incoming {
        use super::*;

        #[rstest]
        #[tokio::test]
        async fn then_return_ok(mut connector: Connector) {
            // given
            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                .can_get_joined_rooms_of_syncer()
                .can_associate_source_group_id_to_room()
                .can_get_users()
                .into();

            // when
            let full_sync_incoming_result = connector.full_sync_incoming().await;

            // then
            assert!(full_sync_incoming_result.is_ok());
        }

        #[rstest]
        #[tokio::test]
        async fn then_add_groups_to_group_mapping(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().build();

            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
            let group_id_mapping = connector.mappings.group_id_mapping.get_group_id_mapping();
            assert_eq!(group_id_mapping.len(), 2);
            assert_eq!(group_id_mapping.get(&room1.source_room_id).unwrap(), &room1.matrix_room_id);
            assert_eq!(group_id_mapping.get(&room2.source_room_id).unwrap(), &room2.matrix_room_id);
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

            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                .with_users(vec![user1.clone(), user2.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_users()
                .can_get_source_user_id_for_all_matrix_users()
                .can_get_joined_rooms_of_user(&user1, vec![])
                .can_get_joined_rooms_of_user(&user2, vec![])
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            let user_id_mapping = connector.mappings.user_id_mapping.get_user_id_mapping();
            assert_eq!(user_id_mapping.len(), 2);
            assert_eq!(
                *user_id_mapping.get(&user1.source_user_id).unwrap(),
                connector
                    .synapse_interactor
                    .get_user_from_mas_user(SynapseApiMocker::get_user_from(&user1))
                    .await
                    .unwrap()
            );
            assert_eq!(
                *user_id_mapping.get(&user2.source_user_id).unwrap(),
                connector
                    .synapse_interactor
                    .get_user_from_mas_user(SynapseApiMocker::get_user_from(&user2))
                    .await
                    .unwrap()
            );
            assert_eq!(
                connector.all_users().await.unwrap(),
                std::collections::HashSet::from([user1.source_user_id, user2.source_user_id])
            );
        }

        #[rstest]
        #[tokio::test]
        async fn then_completely_clears_mappings(mut connector: Connector) {
            // given
            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID).can_get_joined_rooms_of_syncer().can_get_users().into();

            connector.mappings.group_id_mapping.get_group_id_mapping_mut().insert(
                kids_test_lib::util::constants::DEFAULT_SOURCE_GROUP_ID.to_string(),
                kids_test_lib::util::constants::DEFAULT_TARGET_ROOM_ID.to_string(),
            );
            connector.mappings.user_id_mapping.get_user_id_mapping_mut().insert(
                kids_test_lib::util::constants::DEFAULT_SOURCE_USER_ID.to_string(),
                crate::target::types::User {
                    matrix_user_id: std::str::FromStr::from_str(kids_test_lib::util::constants::DEFAULT_TARGET_USER_ID).unwrap(),
                    mas_user_id: "".into(),
                    source_user_id: None,
                    deactivated: false,
                    state: crate::target::types::UserState {
                        display_name: None,
                        emails: vec![],
                        locked: false,
                        is_admin: false,
                        rooms: vec![],
                    },
                },
            );

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert!(connector.mappings.user_id_mapping.get_user_id_mapping().is_empty());
            assert!(connector.mappings.group_id_mapping.get_group_id_mapping().is_empty());
        }

        #[rstest]
        #[tokio::test]
        async fn and_group_mapping_ambiguous_then_error(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().source_room_id(room1.source_room_id.clone()).build();

            let user1 = MockSynapseUserBuilder::default().build();
            let user2 = MockSynapseUserBuilder::default().build();

            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
            let full_sync_result = connector.full_sync_incoming().await;

            // then
            full_sync_result.expect_err("Full sync should fail on duplicate group mappings");
        }

        #[rstest]
        #[tokio::test]
        async fn and_user_mapping_ambiguous_then_error(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().build();

            let user1 = MockSynapseUserBuilder::default().build();
            let user2 = MockSynapseUserBuilder::default().source_user_id(user1.source_user_id.clone()).build();

            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                .with_rooms(vec![room1.clone(), room2.clone()])
                .with_users(vec![user1.clone(), user2.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_joined_rooms_of_user(&user1, vec![])
                .can_get_joined_rooms_of_user(&user2, vec![])
                .can_get_room_associated_source_group_id_v1()
                .can_associate_source_group_id_to_room()
                .can_get_all_rooms_associated_source_group_id()
                .can_get_users()
                .can_get_source_user_id_for_all_matrix_users()
                .into();

            // when
            let full_sync_result = connector.full_sync_incoming().await;

            // then
            full_sync_result.expect_err("Full sync should fail on duplicate user mappings");
        }

        #[rstest]
        #[tokio::test]
        async fn and_obtaining_mapping_for_one_room_fails_then_error(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().build();
            let room3 = MockSynapseRoomBuilder::default().build();

            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
            let full_sync_result = connector.full_sync_incoming().await;

            // then
            full_sync_result.expect_err("Full sync should fail when getting mapping for a room fails");
        }

        #[rstest]
        #[tokio::test]
        async fn and_obtaining_mapping_for_one_user_fails_then_error(mut connector: Connector) {
            // given
            let user1 = MockSynapseUserBuilder::default().build();
            let user2 = MockSynapseUserBuilder::default().build();
            let user3 = MockSynapseUserBuilder::default().build();

            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                .with_users(vec![user1.clone(), user2.clone(), user3.clone()])
                .can_get_joined_rooms_of_syncer()
                .can_get_users()
                .can_get_source_user_id_for_matrix_user(&user1)
                .cannot_get_source_user_id_for_matrix_user(&user2)
                .can_get_source_user_id_for_matrix_user(&user3)
                .can_get_joined_rooms_of_user(&user1, vec![])
                .can_get_joined_rooms_of_user(&user2, vec![])
                .can_get_joined_rooms_of_user(&user3, vec![])
                .into();

            // when
            let full_sync_incoming_result = connector.full_sync_incoming().await;

            // then
            full_sync_incoming_result.expect_err("We do not want partial results");
        }

        #[rstest]
        #[tokio::test]
        async fn but_cannot_get_joined_rooms_of_syncer_then_return_err(mut connector: Connector) {
            // given
            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID).cannot_get_joined_rooms_of_syncer().into();

            // when
            let full_sync_incoming_result = connector.full_sync_incoming().await;

            // then
            assert!(full_sync_incoming_result.is_err());
        }

        #[rstest]
        #[tokio::test]
        async fn but_cannot_get_users_then_return_err(mut connector: Connector) {
            // given
            connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID).can_get_joined_rooms_of_syncer().cannot_get_users().into();

            // when
            let full_sync_incoming_result = connector.full_sync_incoming().await;

            // then
            assert!(full_sync_incoming_result.is_err());
        }
    }

    mod manage_groups {
        use super::*;

        mod create {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn create_group_succeeds_without_attribute(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new("group", None);
                let group_id = group.id.clone();
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                {
                    // 1. Add
                    connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
                    connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
        }

        mod update {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn update_group_succeeds_updates_room_existence(mut connector: Connector) {
                // given
                let mut group = kids_test_lib::Group::new("group", None);
                let group_id = group.id.clone();
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
                let mut group = kids_test_lib::Group::new(
                    // Complex pattern for `DERIVE_DISPLAY_NAME_FROM_GROUP_NAME` handling.
                    "group-Group_Group group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                let matrix_room_id = {
                    // 1. Add
                    connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
                    connector.mappings.group_id_mapping.get_group(&group_id)
                }
                .to_owned();
                {
                    // 2. Update group name
                    let new_group_name = attr;
                    group.attributes.entry(connector.config.source_room_name_attr.clone()).and_modify(|entry| {
                        entry.clear();
                        entry.push(new_group_name.to_owned());
                    });
                    connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                        .with_rooms(vec![
                            MockSynapseRoomBuilder::default()
                                .source_room_id(group_id.clone())
                                .matrix_room_id(matrix_room_id.clone())
                                .build(),
                        ])
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
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
            #[case(crate::target::RoomDeletionStrategy::KickAll)]
            #[tokio::test]
            #[case(crate::target::RoomDeletionStrategy::Evacuate)]
            #[tokio::test]
            #[case(crate::target::RoomDeletionStrategy::Delete)]
            async fn delete_group_kickall_evacuate_delete_room_deletion(
                mut connector: Connector,
                #[case] deletion_strategy: crate::target::RoomDeletionStrategy,
            ) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.config.room_deletion_strategy = deletion_strategy;
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
                    connector.mappings.group_id_mapping.get_group(&group_id)
                };
                connector.synapse_interactor = {
                    let mut mock_api = SynapseApiMocker::new(SYNCER_USER_ID).with_rooms(vec![
                        MockSynapseRoomBuilder::default()
                            .source_room_id(group_id.clone())
                            .matrix_room_id(matrix_room_id.clone())
                            .build(),
                    ]);
                    if matches!(
                        deletion_strategy,
                        crate::target::RoomDeletionStrategy::KickAll | crate::target::RoomDeletionStrategy::Evacuate
                    ) {
                        // Managing room members is necessary to kick users.
                        mock_api = mock_api.can_manage_room_members(
                            matrix_room_id,
                            ["@user-1:matrix.example.com", "@user-2:matrix.example.com"],
                            matches!(deletion_strategy, crate::target::RoomDeletionStrategy::Evacuate),
                            None,
                        );
                        if matches!(deletion_strategy, crate::target::RoomDeletionStrategy::Evacuate) {
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
            #[case(crate::target::RoomDeletionStrategy::KickAll)]
            #[tokio::test]
            #[case(crate::target::RoomDeletionStrategy::Evacuate)]
            async fn delete_group_kickall_evacuate_room_deletion_fails_without_kicking(
                mut connector: Connector,
                #[case] deletion_strategy: crate::target::RoomDeletionStrategy,
            ) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let group_id = group.id.clone();
                connector.config.room_deletion_strategy = deletion_strategy;
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
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
                    connector.mappings.group_id_mapping.get_group(&group_id)
                }
                .to_owned();
                connector.synapse_interactor = {
                    let mut mock_api = SynapseApiMocker::new(SYNCER_USER_ID).with_rooms(vec![
                        MockSynapseRoomBuilder::default()
                            .source_room_id(group_id.clone())
                            .matrix_room_id(matrix_room_id.clone())
                            .build(),
                    ]);
                    // Managing room members is necessary to kick users.
                    // We disallow the syncer to leave the room as we will fail kicking all users.
                    // In that case, the syncer must not leave the room.
                    mock_api = mock_api.can_manage_room_members(
                        matrix_room_id.clone(),
                        ["@user-1:matrix.example.com", "@user-2:matrix.example.com"],
                        false,
                        Some("@user-1:matrix.example.com"),
                    );
                    mock_api.into()
                };
                {
                    // when
                    let deleted = connector.delete_group(&group_id).await;

                    // then
                    let err: KidsError = deleted.expect_err("Deleting group unexpectedly succeeded");
                    match err {
                        KidsError::InternalError(ref err) if err.to_string() == format!("Could not kick all members from room {matrix_room_id}") => {}
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

        mod create {
            use super::*;

            #[rstest]
            #[tokio::test]
            #[case(true)]
            #[tokio::test]
            #[case(false)]
            async fn create_user_succeeds(mut connector: Connector, #[case] is_admin: bool) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let user_builder = kids_test_lib::User::builder()
                    .id("my-sub")
                    .username("firstname.lastname")
                    .first_name("Firstname")
                    .last_name("Lastname")
                    .enabled(true)
                    .with_group(group.clone())
                    .with_role(REQUIRED_ROLE);
                let user = if is_admin {
                    user_builder.with_role(ADMIN_ROLE).build()
                } else {
                    user_builder.build()
                };
                let user_id = user.id.clone();
                let matrix_user = MockSynapseUserBuilder::default()
                    .source_user_id(user_id.clone())
                    .matrix_user_id(format!("@firstname.lastname:{}", kids_test_lib::util::constants::DEFAULT_MATRIX_HOMESERVER))
                    .build();
                let room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                connector
                    .replace_api_mock({
                        let mocker = SynapseApiMocker::new(SYNCER_USER_ID)
                            .with_rooms(vec![room.clone()])
                            .can_get_homeserver_domain(std::str::FromStr::from_str(kids_test_lib::util::constants::DEFAULT_MATRIX_HOMESERVER).unwrap())
                            .can_get_joined_rooms_of_syncer()
                            .can_get_users()
                            .can_get_source_user_id_for_all_matrix_users()
                            .can_get_room_associated_source_group_id_v1()
                            .can_associate_source_group_id_to_room()
                            .can_get_all_rooms_associated_source_group_id()
                            .require_create_user(matrix_user.clone())
                            .can_get_user_display_name(&matrix_user, None)
                            .require_set_user_display_name(&matrix_user, "Firstname Lastname")
                            .can_get_user_emails(&matrix_user, None)
                            .can_get_joined_rooms_of_user(&matrix_user, vec![])
                            .require_join_user_to_room(&matrix_user, &room);
                        if is_admin { mocker.require_set_admin(&matrix_user) } else { mocker }
                    })
                    .await;

                // when
                let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
            }

            #[rstest]
            #[tokio::test]
            async fn create_user_ignores_missing_role(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let user = kids_test_lib::User::builder()
                    .id("my-sub")
                    .username("firstname.lastname")
                    .first_name("Firstname")
                    .last_name("Lastname")
                    .enabled(true)
                    .with_group(group.clone())
                    .build();
                let user_id = user.id.clone();
                let room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                    .with_rooms(vec![room.clone()])
                    .can_get_homeserver_domain("testing.example.com".into())
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
                assert!(!all_users.contains(&user_id));
            }

            #[rstest]
            #[tokio::test]
            async fn not_create_user_succeeds_adding_user(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = kids_test_lib::User::builder()
                    .id("user")
                    .enabled(true)
                    .with_group(group.clone())
                    .with_role(REQUIRED_ROLE)
                    .build();
                let user_id = user.id;
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector
                    .replace_api_mock(
                        SynapseApiMocker::new(SYNCER_USER_ID)
                            .with_rooms(vec![synapse_room.clone()])
                            .with_users(vec![synapse_user.clone()])
                            .can_get_joined_rooms_of_syncer()
                            .can_get_users()
                            .can_get_source_user_id_for_all_matrix_users()
                            .can_get_room_associated_source_group_id_v1()
                            .can_associate_source_group_id_to_room()
                            .can_get_joined_rooms_of_user(&synapse_user, vec![&synapse_room])
                            .can_get_all_rooms_associated_source_group_id(),
                    )
                    .await;

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
                let mut user = kids_test_lib::User::builder()
                    .id("user")
                    .first_name(current_first_name)
                    .last_name("Lastname")
                    .email(current_email)
                    .enabled(true)
                    .with_role(REQUIRED_ROLE)
                    .build();
                let new_first_name = "New First";
                let new_display_name = "New First Lastname";
                let new_email = "my-new-email@example.com";
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector
                    .replace_api_mock(
                        SynapseApiMocker::new(SYNCER_USER_ID)
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
                                    use kids_lib::interface::source::User;
                                    user.display_name().unwrap()
                                }),
                            )
                            .can_get_user_emails(&synapse_user, Some(current_email.to_owned())),
                    )
                    .await;
                let created = connector.create_or_update_user(std::sync::Arc::new(user.clone())).await;
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));

                // when
                let previous_display_name = {
                    use kids_lib::interface::source::User;
                    user.display_name().unwrap()
                };
                user.first_name = Some(new_first_name.to_owned());
                user.email = Some(new_email.to_owned());
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                    .with_rooms(vec![])
                    .with_users(vec![synapse_user.clone()])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    // The user is no member of any (managed) room.
                    .can_get_joined_rooms_of_user(&synapse_user, vec![])
                    .can_get_user_display_name(&synapse_user, Some(previous_display_name))
                    .can_get_user_emails(&synapse_user, Some(current_email.to_owned()))
                    .require_set_user_display_name(&synapse_user, new_display_name)
                    .require_set_user_email(&synapse_user, new_email)
                    .into();
                let updated = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                updated.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
            }

            #[rstest]
            #[tokio::test]
            async fn update_user_locks_without_role_unlocks_with_role(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let mut user = kids_test_lib::User::builder()
                    .id("user")
                    .first_name("First")
                    .last_name("Lastname")
                    .enabled(true)
                    .with_group(group.clone())
                    .with_role(REQUIRED_ROLE)
                    .build();
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector
                    .replace_api_mock(
                        SynapseApiMocker::new(SYNCER_USER_ID)
                            .with_rooms(vec![synapse_room.clone()])
                            .with_users(vec![synapse_user.clone()])
                            .can_get_room_associated_source_group_id_v1()
                            .can_associate_source_group_id_to_room()
                            .can_get_room_associated_source_group_id_for_room(&synapse_room)
                            .can_get_joined_rooms_of_syncer()
                            .can_get_users()
                            .can_get_source_user_id_for_all_matrix_users()
                            // The user is no member of any (managed) room.
                            .can_get_joined_rooms_of_user(&synapse_user, vec![])
                            .can_get_user_display_name(
                                &synapse_user,
                                Some({
                                    use kids_lib::interface::source::User;
                                    user.display_name().unwrap()
                                }),
                            )
                            .can_get_user_emails(&synapse_user, None)
                            .require_join_user_to_room(&synapse_user, &synapse_room),
                    )
                    .await;
                let created = connector.create_or_update_user(std::sync::Arc::new(user.clone())).await;
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));

                // when
                user.roles = vec![];
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                    .with_rooms(vec![synapse_room.clone()])
                    .with_users(vec![synapse_user.clone()])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    .require_lock_user(&synapse_user)
                    .can_get_joined_rooms_of_user(&synapse_user, vec![&synapse_room])
                    .can_get_user_display_name(
                        &synapse_user,
                        Some({
                            use kids_lib::interface::source::User;
                            user.display_name().unwrap()
                        }),
                    )
                    .can_get_user_emails(&synapse_user, None)
                    .require_kick_user_from_room(&synapse_user, &synapse_room)
                    .into();
                let updated = connector.create_or_update_user(std::sync::Arc::new(user.clone())).await;

                // then
                updated.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));

                // when
                user.roles = vec![REQUIRED_ROLE.to_owned()];
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                    .with_rooms(vec![synapse_room.clone()])
                    .with_users(vec![synapse_user.clone()])
                    .can_get_joined_rooms_of_syncer()
                    .can_get_users()
                    .can_get_source_user_id_for_all_matrix_users()
                    .require_unlock_user(&synapse_user)
                    .can_get_user_display_name(
                        &synapse_user,
                        Some({
                            use kids_lib::interface::source::User;
                            user.display_name().unwrap()
                        }),
                    )
                    .can_get_user_emails(&synapse_user, None)
                    // The user is no member of any (managed) room.
                    .can_get_joined_rooms_of_user(&synapse_user, vec![])
                    .require_join_user_to_room(&synapse_user, &synapse_room)
                    .into();
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
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = kids_test_lib::User::builder()
                    .id("user")
                    .enabled(true)
                    .with_group(group.clone())
                    .with_role(REQUIRED_ROLE)
                    .build();
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector
                    .replace_api_mock(
                        SynapseApiMocker::new(SYNCER_USER_ID)
                            .with_rooms(vec![synapse_room.clone()])
                            .with_users(vec![synapse_user.clone()])
                            .can_get_joined_rooms_of_syncer()
                            .can_get_users()
                            .can_get_source_user_id_for_all_matrix_users()
                            .can_get_room_associated_source_group_id_v1()
                            .can_associate_source_group_id_to_room()
                            .can_get_all_rooms_associated_source_group_id()
                            .can_get_user_display_name(&synapse_user, None)
                            .can_get_user_emails(&synapse_user, None)
                            // Assume the user is not yet member of any (managed) room.
                            .can_get_joined_rooms_of_user(&synapse_user, vec![])
                            // This is the core assertion here: The user gets added to the room.
                            .require_join_user_to_room(&synapse_user, &synapse_room),
                    )
                    .await;

                // when
                let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
                let user = connector.mappings.user_id_mapping.get_user(&user_id);
                assert_eq!(user.state.rooms, vec![synapse_room.matrix_room_id]);
            }

            #[rstest]
            #[tokio::test]
            async fn update_user_leaves_room(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = kids_test_lib::User::builder().id("user").enabled(true).with_role(REQUIRED_ROLE).build();
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector
                    .replace_api_mock(
                        SynapseApiMocker::new(SYNCER_USER_ID)
                            .with_rooms(vec![synapse_room.clone()])
                            .with_users(vec![synapse_user.clone()])
                            .can_get_joined_rooms_of_syncer()
                            .can_get_users()
                            .can_get_source_user_id_for_all_matrix_users()
                            .can_get_room_associated_source_group_id_v1()
                            .can_associate_source_group_id_to_room()
                            .can_get_all_rooms_associated_source_group_id()
                            .can_get_user_display_name(&synapse_user, None)
                            .can_get_user_emails(&synapse_user, None)
                            // Assume the user is still member of the managed room.
                            .can_get_joined_rooms_of_user(&synapse_user, vec![&synapse_room])
                            // This is the core assertion here: The user gets kicked from the room.
                            .require_kick_user_from_room(&synapse_user, &synapse_room),
                    )
                    .await;

                // when
                let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                // then
                created.expect("Error creating or updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.contains(&user_id));
                let user = connector.mappings.user_id_mapping.get_user(&user_id);
                assert_eq!(user.state.rooms, Vec::<String>::new());
            }

            #[rstest]
            #[tokio::test]
            async fn update_user_locking(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let mut user = kids_test_lib::User::builder()
                    .id("user")
                    .enabled(true)
                    .with_group(group.clone())
                    .with_role(REQUIRED_ROLE)
                    .build();
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                let get_api_mocker = |joined_room: Vec<&crate::target::test_mocks::MockSynapseRoom>| {
                    SynapseApiMocker::new(SYNCER_USER_ID)
                        .with_rooms(vec![synapse_room.clone()])
                        .with_users(vec![synapse_user.clone()])
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        .can_get_source_user_id_for_all_matrix_users()
                        .can_get_room_associated_source_group_id_v1()
                        .can_associate_source_group_id_to_room()
                        .can_get_all_rooms_associated_source_group_id()
                        .can_get_user_display_name(&synapse_user, None)
                        .can_get_user_emails(&synapse_user, None)
                        .can_get_joined_rooms_of_user(&synapse_user, joined_room)
                };
                {
                    // 1. Create as unlocked.
                    // when
                    connector.replace_api_mock(get_api_mocker(vec![&synapse_room])).await;

                    // then
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.mappings.user_id_mapping.get_user(&user_id);
                    assert!(!user_via_connector.state.locked);
                }
                {
                    // 2. Update to locked.
                    user.enabled = false;
                    connector.synapse_interactor = get_api_mocker(vec![&synapse_room])
                        .require_lock_user(&synapse_user)
                        .require_kick_user_from_room(&synapse_user, &synapse_room)
                        .into();

                    // when
                    let created = connector.create_or_update_user(std::sync::Arc::new(user.clone())).await;

                    // then
                    created.expect("Error creating or updating user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.mappings.user_id_mapping.get_user(&user_id);
                    assert!(user_via_connector.state.locked);
                    assert_eq!(user_via_connector.state.rooms, Vec::<String>::new());
                }
                {
                    // 3. Update to unlocked.
                    user.enabled = true;
                    connector.synapse_interactor = get_api_mocker(vec![])
                        .require_unlock_user(&synapse_user)
                        .require_join_user_to_room(&synapse_user, &synapse_room)
                        .into();

                    // when
                    let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                    // then
                    created.expect("Error creating or updating user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.mappings.user_id_mapping.get_user(&user_id);
                    assert!(!user_via_connector.state.locked);
                    assert_eq!(user_via_connector.state.rooms, vec![synapse_room.matrix_room_id]);
                }
            }

            #[rstest]
            #[tokio::test]
            async fn update_user_admin(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let mut user = kids_test_lib::User::builder()
                    .id("user")
                    .enabled(true)
                    .with_group(group.clone())
                    .with_role(REQUIRED_ROLE)
                    .build();
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                let get_api_mocker = || {
                    SynapseApiMocker::new(SYNCER_USER_ID)
                        .with_rooms(vec![synapse_room.clone()])
                        .with_users(vec![synapse_user.clone()])
                        .can_get_joined_rooms_of_syncer()
                        .can_get_users()
                        .can_get_source_user_id_for_all_matrix_users()
                        .can_get_room_associated_source_group_id_v1()
                        .can_associate_source_group_id_to_room()
                        .can_get_all_rooms_associated_source_group_id()
                        .can_get_user_display_name(&synapse_user, None)
                        .can_get_user_emails(&synapse_user, None)
                        .can_get_joined_rooms_of_user(&synapse_user, vec![&synapse_room])
                };
                {
                    // 1. Create as non-admin.
                    // when
                    connector.replace_api_mock(get_api_mocker()).await;

                    // then
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.mappings.user_id_mapping.get_user(&user_id);
                    assert!(!user_via_connector.state.is_admin);
                }
                {
                    // 2. Update to admin.
                    user.roles.push(ADMIN_ROLE.to_owned());
                    connector.synapse_interactor = get_api_mocker().require_set_admin(&synapse_user).into();

                    // when
                    let created = connector.create_or_update_user(std::sync::Arc::new(user.clone())).await;

                    // then
                    created.expect("Error creating or updating user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.mappings.user_id_mapping.get_user(&user_id);
                    assert!(user_via_connector.state.is_admin);
                }
                {
                    // 3. Update to non-admin.
                    user.roles.pop();
                    connector.synapse_interactor = get_api_mocker().require_remove_admin(&synapse_user).into();

                    // when
                    let created = connector.create_or_update_user(std::sync::Arc::new(user)).await;

                    // then
                    created.expect("Error creating or updating user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.mappings.user_id_mapping.get_user(&user_id);
                    assert!(!user_via_connector.state.is_admin);
                }
            }
        }

        mod delete {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn delete_user_locks_it(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = kids_test_lib::User::builder()
                    .id("user")
                    .first_name("User")
                    .email("user@example.com")
                    .enabled(true)
                    .with_group(group.clone())
                    .with_role(REQUIRED_ROLE)
                    .with_role(ADMIN_ROLE)
                    .build();
                let user_id = user.id.clone();
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                {
                    // 1. Create user.
                    connector
                        .replace_api_mock(
                            SynapseApiMocker::new(SYNCER_USER_ID)
                                .with_rooms(vec![synapse_room.clone()])
                                .with_users(vec![synapse_user.clone()])
                                .can_get_joined_rooms_of_syncer()
                                .can_get_users()
                                .can_get_source_user_id_for_all_matrix_users()
                                .can_get_room_associated_source_group_id_v1()
                                .can_associate_source_group_id_to_room()
                                .can_get_joined_rooms_of_user(&synapse_user, vec![&synapse_room])
                                .require_set_user_display_name(&synapse_user, "User")
                                .require_set_user_email(&synapse_user, "user@example.com")
                                .require_set_admin(&synapse_user)
                                .can_get_all_rooms_associated_source_group_id(),
                        )
                        .await;
                    connector.create_or_update_user(std::sync::Arc::new(user)).await.unwrap();
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                }
                {
                    // 2. Delete user.
                    connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                        .with_rooms(vec![synapse_room.clone()])
                        .with_users(vec![synapse_user.clone()])
                        .can_get_joined_rooms_of_syncer()
                        .can_get_joined_rooms_of_user(&synapse_user, vec![&synapse_room])
                        .require_unset_user_email(&synapse_user)
                        .require_lock_user(&synapse_user)
                        .require_remove_admin(&synapse_user)
                        .require_kick_user_from_room(&synapse_user, &synapse_room)
                        .into();

                    // when
                    let deleted = connector.delete_user(&user_id).await;

                    // then
                    deleted.expect("Error deleting user");
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user = connector.mappings.user_id_mapping.get_user(&user_id);
                    assert_eq!(
                        user.state,
                        crate::target::types::UserState {
                            display_name: Some("User".to_owned()),
                            emails: vec![],
                            locked: true,
                            is_admin: false,
                            rooms: vec![]
                        }
                    );
                }
            }
        }
    }
}
