use std::collections;

use kids_lib::error::KidsError;

use crate::target::{dto, external};

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
    async fn ensure_user_display_name(
        &mut self,
        matrix_user_id: &str,
        source_user: &(dyn kids_lib::interface::source::User + Send + Sync),
    ) -> Result<(), KidsError> {
        let desired_display_name = source_user.display_name();
        self.synapse_interactor
            .ensure_user_display_name(matrix_user_id, desired_display_name.as_deref(), source_user.id())
            .await
    }

    async fn ensure_user_email(&mut self, matrix_user_id: &str, source_user: &(dyn kids_lib::interface::source::User + Send + Sync)) -> Result<(), KidsError> {
        let desired_email = source_user.email();
        self.synapse_interactor.ensure_user_email(matrix_user_id, desired_email, source_user.id()).await
    }

    /// Returns `true` when the user has the required role in Source
    /// or when the the config option `required_role_name` is unset.
    async fn source_user_has_required_role(&self, source_user: &(dyn kids_lib::interface::source::User + Send + Sync)) -> Result<bool, KidsError> {
        if let Some(required_role_name) = &self.config.required_role_name {
            let roles = source_user.roles().await?;
            let required_role_present = roles.contains(required_role_name);
            return Ok(required_role_present);
        }
        // If no required role is set, pass this test
        Ok(true)
    }

    async fn ensure_user_locked_state_in_sync(
        &mut self,
        source_user: &(dyn kids_lib::interface::source::User + Send + Sync),
        enforce_lock: bool,
    ) -> Result<(), KidsError> {
        match source_user.enabled() && !enforce_lock {
            // Note that we explicitly want to lock users here, NOT deactivate them.
            // Deactivating users appears to delete all keys of that user, so even when a
            // user is reactivated, they cannot log in with the same identity and lose
            // all of their direct message rooms.
            // With locking, this works properly and unlocked users will encounter the same
            // state they left off with before being locked.
            false => self.ensure_user_locked(source_user.id()).await,
            true => self.ensure_user_unlocked(source_user.id()).await,
        }
    }

    async fn ensure_user_locked(&mut self, source_user_id: &str) -> Result<(), KidsError> {
        let matrix_user = self.mappings.get_user(source_user_id);
        let matrix_user_id = matrix_user.name.as_str();
        if !matrix_user.locked {
            // Note that we explicitly want to lock users here, NOT deactivate them.
            // Deactivating users appears to delete all keys of that user, so even when a
            // user is reactivated, they cannot log in with the same identity and lose
            // all of their direct message rooms.
            // With locking, this works properly and unlocked users will encounter the same
            // state they left off with before being locked.
            match self.synapse_interactor.synapse_api().lock_user(matrix_user_id).await {
                Ok(()) => {
                    // Write lock state to user object.
                    // Note: `user` and `matrix_user` represent the exact same entity.
                    let user = self.mappings.get_user_mut(source_user_id);
                    user.locked = true;
                    tracing::info!(matrix_user_id = user.name, "Locked user");
                }
                Err(e) => tracing::warn!(?e, matrix_user_id, "Could not lock user"),
            };
        }
        Ok(())
    }

    async fn ensure_user_unlocked(&mut self, source_user_id: &str) -> Result<(), KidsError> {
        let matrix_user = self.mappings.get_user(source_user_id);
        let matrix_user_id = matrix_user.name.as_str();
        if matrix_user.locked {
            match self.synapse_interactor.synapse_api().unlock_user(matrix_user_id).await {
                Ok(()) => {
                    // Write lock state to user object.
                    // Note: `user` and `matrix_user` represent the exact same entity.
                    let user = self.mappings.get_user_mut(source_user_id);
                    user.locked = false;
                    tracing::info!(matrix_user_id = user.name, "Unlocked user");
                }
                Err(e) => tracing::warn!(?e, matrix_user_id, "Could not unlock user"),
            };
        }
        Ok(())
    }

    async fn ensure_user_rooms(&mut self, source_user: &(dyn kids_lib::interface::source::User + Send + Sync)) -> Result<(), KidsError> {
        let matrix_user_id = self.mappings.get_user(source_user.id()).name.as_str();

        let desired_user_groups = source_user
            .groups(true)
            .await
            .map_err(|e| e.with_context(&format!("Could not get source groups associated with source user {}", source_user.id())))?;
        let current_user_rooms = self
            .synapse_interactor
            .synapse_api()
            .get_user_joined_rooms(matrix_user_id)
            .await
            .map_err(|e| e.with_context(&format!("Could not get matrix rooms user {matrix_user_id} has currently joined")))?;

        let desired_user_rooms: Vec<&String> = if !self.mappings.get_user(source_user.id()).locked {
            desired_user_groups
                .iter()
                .filter_map(|group| {
                    // We only want to add the user to groups that have a corresponding matrix room.
                    // Note: Since rooms are being created before users, all valid rooms must be contained
                    // in the mapping at this point.
                    self.mappings.get_group_opt(group.id())
                })
                .collect()
        } else {
            // If user is not enabled, we want to remove it from all rooms it is in.
            // Simply clearing the desired rooms will have this effect using the logic below.
            vec![]
        };

        // Add user to all desired groups that they are not already joined to.
        for matrix_room_id in &desired_user_rooms {
            if !current_user_rooms.joined_rooms.contains(matrix_room_id) {
                match self.synapse_interactor.synapse_api().join_user_to_room(matrix_room_id, matrix_user_id).await {
                    Ok(()) => tracing::info!(matrix_room_id, matrix_user_id, "User joined matrix room"),
                    Err(e) => tracing::warn!(?e, matrix_room_id, matrix_user_id, "Could not join user to matrix room"),
                }
            } else {
                tracing::trace!(matrix_room_id, matrix_user_id, "User has already joined matrix room");
            }
        }

        // Remove user from all joined groups that are no longer desired.
        for matrix_room_id in &current_user_rooms.joined_rooms {
            if !desired_user_rooms.contains(&matrix_room_id) {
                match self.synapse_interactor.synapse_api().kick_user_from_room(matrix_room_id, matrix_user_id).await {
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

#[async_trait::async_trait]
impl kids_lib::interface::target::Target for Connector {
    type Config = SynapseConfig;

    async fn new(config: Self::Config) -> Result<Self, KidsError> {
        let synapse_api = external::SynapseClient::new(config.synapse_api.clone())
            .await
            .map_err(|e| e.with_context("Failed to create Synapse API client"))?;
        let mut synapse_interactor = crate::target::SynapseInteractor::new(synapse_api);
        let mappings = crate::target::IdMapping::generate(&mut synapse_interactor).await?;
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
        self.mappings = crate::target::IdMapping::generate(&mut self.synapse_interactor).await?;

        Ok(())
    }

    /// Return the identifiers of all [Source Groups](kids_lib::interface::source::Group) known to Synapse.
    /// These are exactly the ones we managed to obtain a mapping to a Matrix room for earlier.
    /// There might be additional rooms in Synapse not mapped to a Source group, which will not be considered in the result of this method.
    async fn all_groups(&mut self) -> Result<collections::HashSet<kids_lib::types::SharedResourceIdentifier>, KidsError> {
        Ok(self.mappings.get_group_id_mapping().keys().cloned().collect())
    }

    async fn all_users(&mut self) -> Result<collections::HashSet<kids_lib::types::SharedResourceIdentifier>, KidsError> {
        Ok(self.mappings.get_user_id_mapping().keys().cloned().collect())
    }

    async fn delete_group(&mut self, source_group_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), KidsError> {
        let matrix_room_id = match self.mappings.get_group_opt(source_group_id) {
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

        self.mappings.get_group_id_mapping_mut().remove(source_group_id);

        Ok(())
    }

    async fn delete_user(&mut self, user_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), KidsError> {
        if let Some(syncer_source_user_id) = self.mappings.get_syncer_source_user_id()
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

        let matrix_user = match self.mappings.get_user_opt(user_id) {
            Some(matrix_user) => matrix_user,
            None => {
                // This should not happen, as the controller should only attempt to delete users that
                // we told it exists in Matrix before via the `self.all_users` method.
                tracing::warn!(source_user_id = user_id, "Cannot deactivate source user, because it is not known to Matrix");
                return Ok(());
            }
        };

        let matrix_user_id = matrix_user.name.as_str();
        tracing::info!(matrix_user_id, "Deactivating matrix user");
        self.synapse_interactor
            .synapse_api()
            .deactivate_user(matrix_user_id)
            .await
            .map_err(|e| e.with_context(&format!("Could not deactivate matrix user {matrix_user_id}")))?;
        self.mappings.get_user_id_mapping_mut().remove(user_id);
        Ok(())
    }

    async fn create_or_update_group(&mut self, source_group: std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>) -> Result<(), KidsError> {
        // Note that groups containing the below-mentioned characters will lead to ambiguitive group paths,
        // which is why we do not allow them.
        // For example, a subgroup "B" of group "A" will receive the path "/A/B", but so will a group named "A/B" directly.
        // The colon causes issues because it is used as a delimiter in the matrix room alias.
        if source_group.name().contains(":") || source_group.name().contains("/") {
            return Err(KidsError::InternalError(format!(
                "Could not create room for group {}: group name contains invalid character",
                source_group.id()
            )));
        }

        // The target does only care about groups with the domain-specific attribute.
        let room_name_attr = match self.get_room_name_attr(source_group.as_ref()) {
            Some(room_name_attr) => room_name_attr,
            None => {
                // if !source_group.attributes().contains_key(&self.config.source_room_name_attr) {
                match self.mappings.get_group_opt(source_group.id()) {
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

        Ok(())
    }

    async fn create_or_update_user(&mut self, source_user: std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>) -> Result<(), KidsError> {
        if let Some(syncer_source_user_id) = self.mappings.get_syncer_source_user_id()
            && syncer_source_user_id == source_user.id()
        {
            tracing::trace!(source_user_id = source_user.id(), "Ignoring Syncer user.");
            return Ok(());
        }

        let matrix_user_known = self.mappings.has_user(source_user.id());
        let source_user_has_required_role = self.source_user_has_required_role(source_user.as_ref()).await?;
        if !matrix_user_known && !source_user_has_required_role {
            // Exit early and do not create the user as the user is missing the required role.
            // If user already exists, it will be locked later
            tracing::trace!(source_user_id = source_user.id(), "Ignoring user that has no access to Matrix");
            return Ok(());
        }

        // The lifetimes don't work out:
        // In theory, we could use the mutable reference to the user in the mapping
        // and modify it in place (e.g. lock state).
        // In practice, the borrow checker cannot prove that the (mutable) borrows
        // of `self.group_id_mapping` and `self.user_id_mapping` for getting the user
        // and of `self.synapse_api` to interact with Synapse
        // do not interfere with each other.
        // Simply making the API operations immutable is not enough; we cannot borrow `self`
        // even immutably while the mutable borrow from `self.get_or_create_user` is active.
        // You'd need to split the mappings off into its own struct that can be borrowed individually.
        // A first try of this is in https://rechenknecht.net/giz/keycloak/kids/-/tree/feat/synapse-create-users-wip
        // but this turned out to be quite complex and very ugly.
        let matrix_user_id = &self.get_or_create_user(source_user.as_ref()).await?.name.clone();

        // Lock also if user has no requried role
        self.ensure_user_locked_state_in_sync(source_user.as_ref(), !source_user_has_required_role)
            .await?;

        self.ensure_user_display_name(matrix_user_id, source_user.as_ref()).await?;
        self.ensure_user_email(matrix_user_id, source_user.as_ref()).await?;

        self.ensure_user_rooms(source_user.as_ref()).await?;

        Ok(())
    }
}

impl Connector {
    fn generate_matrix_user_id(&self, source_user: &(dyn kids_lib::interface::source::User + Send + Sync)) -> Result<String, KidsError> {
        match source_user.username() {
            Some(username) => Ok(self.synapse_interactor.generate_matrix_user_id(username)),
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

    async fn get_or_create_user(&mut self, source_user: &(dyn kids_lib::interface::source::User + Send + Sync)) -> Result<&mut dto::User, KidsError> {
        // Unfortunately, `match` did not work here for lifetime reasons.
        if !self.mappings.has_user(source_user.id()) {
            let matrix_user_id = self.generate_matrix_user_id(source_user)?;
            tracing::info!(
                source_user_id = source_user.id(),
                source_user_name = source_user.username(),
                matrix_user_id = matrix_user_id,
                "Creating user"
            );
            let matrix_user = self
                .synapse_interactor
                .synapse_api()
                .create_user(matrix_user_id.as_str(), source_user.id())
                .await?;
            self.mappings.get_user_id_mapping_mut().insert(source_user.id().clone(), matrix_user);
        }
        Ok(self.mappings.get_user_opt_mut(source_user.id()).expect("We have just added that user."))
    }

    async fn get_or_create_room(
        &mut self,
        source_group: &std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>,
    ) -> Result<&mut String, KidsError> {
        // Unfortunately, `match` did not work here for lifetime reasons.
        if !self.mappings.has_group(source_group.id()) {
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
            self.mappings.get_group_id_mapping_mut().insert(source_group.id().to_owned(), matrix_room_id);
            tracing::info!(
                source_id = source_group.id(),
                group_name = source_group.name(),
                matrix_room_id = self.mappings.get_group(source_group.id()),
                "Room created"
            );
        }
        Ok(self.mappings.get_group_opt_mut(source_group.id()).expect("We have just added that group."))
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
    async fn update_display_name(&mut self, matrix_room_id: &str, room_name_attr: String, source_group_name: &str) {
        let desired_name = self.get_room_desired_display_name(room_name_attr, source_group_name);
        self.synapse_interactor.ensure_group_display_name(matrix_room_id, desired_name).await;
    }

    async fn update_canonical_alias(&mut self, matrix_room_id: &str, source_group: &std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>) {
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
    const SYNCER_USER_ID: &str = "syncer-user";

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
                room_deletion_strategy: crate::target::RoomDeletionStrategy::Ignore,
                source_room_name_attr: "test".to_string(),
                required_role_name: Some(REQUIRED_ROLE.to_owned()),
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
            let group_id_mapping = connector.mappings.get_group_id_mapping();
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
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            let user_id_mapping = connector.mappings.get_user_id_mapping();
            assert_eq!(user_id_mapping.len(), 2);
            assert_eq!(user_id_mapping.get(&user1.source_user_id).unwrap(), &SynapseApiMocker::get_user_from(&user1));
            assert_eq!(user_id_mapping.get(&user2.source_user_id).unwrap(), &SynapseApiMocker::get_user_from(&user2));
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

            connector.mappings.get_group_id_mapping_mut().insert(
                kids_test_lib::util::constants::DEFAULT_SOURCE_GROUP_ID.to_string(),
                kids_test_lib::util::constants::DEFAULT_TARGET_ROOM_ID.to_string(),
            );
            connector.mappings.get_user_id_mapping_mut().insert(
                kids_test_lib::util::constants::DEFAULT_SOURCE_USER_ID.to_string(),
                dto::User {
                    name: kids_test_lib::util::constants::DEFAULT_TARGET_USER_ID.to_string(),
                    locked: false,
                    external_ids: None,
                    threepids: None,
                },
            );

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert!(connector.mappings.get_user_id_mapping().is_empty());
            assert!(connector.mappings.get_group_id_mapping().is_empty());
        }

        #[rstest]
        #[tokio::test]
        async fn and_mapping_ambiguous_then_use_first_encountered_value(mut connector: Connector) {
            // given
            let room1 = MockSynapseRoomBuilder::default().build();
            let room2 = MockSynapseRoomBuilder::default().source_room_id(room1.source_room_id.clone()).build();

            let user1 = MockSynapseUserBuilder::default().build();
            let user2 = MockSynapseUserBuilder::default().source_user_id(user1.source_user_id.clone()).build();

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
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert_eq!(connector.mappings.get_group(&room1.source_room_id), &room1.matrix_room_id);
            assert_eq!(connector.mappings.get_user(&user1.source_user_id), &SynapseApiMocker::get_user_from(&user1));
        }

        #[rstest]
        #[tokio::test]
        async fn and_obtaining_mapping_for_one_room_fails_then_still_process_other_rooms(mut connector: Connector) {
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
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert!(connector.mappings.has_group(&room1.source_room_id));
            assert!(!connector.mappings.has_group(&room2.source_room_id));
            assert!(connector.mappings.has_group(&room3.source_room_id));
        }

        #[rstest]
        #[tokio::test]
        async fn and_obtaining_mapping_for_one_user_fails_then_still_process_other_user(mut connector: Connector) {
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
                .into();

            // when
            assert!(connector.full_sync_incoming().await.is_ok());

            // then
            assert!(connector.mappings.has_user(&user1.source_user_id));
            assert!(!connector.mappings.has_user(&user2.source_user_id));
            assert!(connector.mappings.has_user(&user3.source_user_id));
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

        impl From<Group> for std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync> {
            fn from(value: Group) -> Self {
                std::sync::Arc::new(value)
            }
        }

        #[async_trait::async_trait(?Send)]
        impl kids_lib::interface::source::Group for Group {
            fn id(&self) -> &kids_lib::types::SharedResourceIdentifier {
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

            fn root_group(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn kids_lib::interface::source::Group> {
                self
            }

            fn parent_group(&self) -> Option<std::sync::Arc<dyn kids_lib::interface::source::Group>> {
                None
            }

            async fn sub_groups(self: std::sync::Arc<Self>) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::Group>>, KidsError> {
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
                let group = Group::new(
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
                let group = Group::new(
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

            #[rstest]
            #[tokio::test]
            #[case("C:\\Windows")]
            #[tokio::test]
            #[case("/usr/bin")]
            async fn create_group_fails_with_invalid_group_name(mut connector: Connector, #[case] invalid_group_name: &'static str) {
                // given
                let group = Group::new(invalid_group_name, None);
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID).can_get_joined_rooms_of_syncer().can_get_users().into();

                // when
                let created = connector.create_or_update_group(std::sync::Arc::new(group)).await;

                // then
                let err: KidsError = created.expect_err("Creating or updating group unexpectedly succeeded");
                match err {
                    KidsError::InternalError(ref msg)
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
                let mut group = Group::new(
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
                    connector.mappings.get_group(&group_id)
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
                let group = Group::new(
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
                let group = Group::new(
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
                    connector.mappings.get_group(&group_id)
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
                            ["user-1", "user-2"],
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
                let group = Group::new(
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
                    connector.mappings.get_group(&group_id)
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
                    mock_api = mock_api.can_manage_room_members(matrix_room_id.clone(), ["user-1", "user-2"], false, Some("user-1"));
                    mock_api.into()
                };
                {
                    // when
                    let deleted = connector.delete_group(&group_id).await;

                    // then
                    let err: KidsError = deleted.expect_err("Deleting group unexpectedly succeeded");
                    match err {
                        KidsError::InternalError(ref msg) if *msg == format!("Could not kick all members from room {matrix_room_id}") => {}
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
            roles: Vec<String>,
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
                roles: Option<Vec<String>>,
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
                    // Use required role if nothing else is put in.
                    roles: roles.unwrap_or(vec![REQUIRED_ROLE.to_owned()]),
                }
            }
        }

        #[async_trait::async_trait]
        impl kids_lib::interface::source::User for User {
            fn id(&self) -> &kids_lib::types::SharedResourceIdentifier {
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
            ) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>>, KidsError> {
                Ok(self.groups.clone().into_iter().map(Into::into).collect())
            }

            async fn roles(&self) -> Result<Vec<String>, KidsError> {
                Ok(self.roles.to_vec())
            }
        }

        /// Note that [`create_or_update_user`](Connector::create_or_update_user) can never create a new user account.
        /// We require that the user handles the first login manually, e.g. to setup the recovery key.
        mod create {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn create_user_succeeds(mut connector: Connector) {
                // given
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let user = User::new(
                    "my-sub",
                    Some("firstname.lastname".to_owned()),
                    Some("Firstname".to_owned()),
                    Some("Lastname".to_owned()),
                    None,
                    true,
                    None,
                    Some(vec![group.clone()]),
                    None,
                );
                let user_id = user.id.clone();
                let matrix_user = MockSynapseUserBuilder::default()
                    .source_user_id(user_id.clone())
                    .matrix_user_id("@firstname.lastname:testing.example.com")
                    .build();
                let room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                connector
                    .replace_api_mock(
                        SynapseApiMocker::new(SYNCER_USER_ID)
                            .with_rooms(vec![room.clone()])
                            .can_get_homeserver_domain("testing.example.com")
                            .can_get_joined_rooms_of_syncer()
                            .can_get_users()
                            .can_get_source_user_id_for_all_matrix_users()
                            .can_get_room_associated_source_group_id_v1()
                            .can_associate_source_group_id_to_room()
                            .can_get_all_rooms_associated_source_group_id()
                            .require_create_user(matrix_user.clone())
                            .can_get_user_display_name(&matrix_user, None)
                            .require_set_user_display_name(&matrix_user, "Firstname Lastname")
                            .can_get_user_three_pids(&matrix_user, None)
                            .can_get_joined_rooms_of_user(&matrix_user, vec![])
                            .require_join_user_to_room(&matrix_user, &room),
                    )
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
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let user = User::new(
                    "my-sub",
                    Some("firstname.lastname".to_owned()),
                    Some("Firstname".to_owned()),
                    Some("Lastname".to_owned()),
                    None,
                    true,
                    None,
                    Some(vec![group.clone()]),
                    Some(vec![]),
                );
                let user_id = user.id.clone();
                let room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID)
                    .with_rooms(vec![room.clone()])
                    .can_get_homeserver_domain("testing.example.com")
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
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = User::new("user", None, None, None, None, true, None, Some(vec![group]), None);
                let user_id = user.id;
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                connector
                    .replace_api_mock(
                        SynapseApiMocker::new(SYNCER_USER_ID)
                            .with_rooms(vec![synapse_room])
                            .with_users(vec![synapse_user])
                            .can_get_joined_rooms_of_syncer()
                            .can_get_users()
                            .can_get_source_user_id_for_all_matrix_users()
                            .can_get_room_associated_source_group_id_v1()
                            .can_associate_source_group_id_to_room()
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
                let mut user = User::new(
                    "user",
                    None,
                    Some(current_first_name.to_owned()),
                    Some("Lastname".to_owned()),
                    Some(current_email.to_owned()),
                    true,
                    None,
                    None,
                    None,
                );
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
                            .can_get_user_three_pids(&synapse_user, Some(current_email.to_owned())),
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
                    .can_get_user_three_pids(&synapse_user, Some(current_email.to_owned()))
                    .require_set_user_display_name(&synapse_user, new_display_name)
                    .require_set_user_three_pids(&synapse_user, new_email)
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
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let current_first_name = "First";
                let mut user = User::new(
                    "user",
                    None,
                    Some(current_first_name.to_owned()),
                    Some("Lastname".to_owned()),
                    None,
                    true,
                    None,
                    Some(vec![group]),
                    None,
                );
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
                            .can_get_user_three_pids(&synapse_user, None)
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
                    .can_get_user_three_pids(&synapse_user, None)
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
                    .can_get_user_three_pids(&synapse_user, None)
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
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = User::new("user", None, None, None, None, true, None, Some(vec![group]), None);
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
                            .can_get_user_three_pids(&synapse_user, None)
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
                let user = User::new("user", None, None, None, None, true, None, None, None);
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
                            .can_get_user_three_pids(&synapse_user, None)
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
                let mut user = User::new("user", None, None, None, None, true, None, Some(vec![group]), None);
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
                        .can_get_user_three_pids(&synapse_user, None)
                        .can_get_joined_rooms_of_user(&synapse_user, joined_room)
                };
                {
                    // 1. Create as unlocked.
                    // when
                    connector.replace_api_mock(get_api_mocker(vec![&synapse_room])).await;

                    // then
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                    let user_via_connector = connector.mappings.get_user(&user_id);
                    assert!(!user_via_connector.locked);
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
                    let user_via_connector = connector.mappings.get_user(&user_id);
                    assert!(user_via_connector.locked);
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
                    let user_via_connector = connector.mappings.get_user(&user_id);
                    assert!(!user_via_connector.locked);
                }
            }
        }

        mod delete {
            use super::*;

            #[rstest]
            #[tokio::test]
            async fn delete_user_deactivates_it(mut connector: Connector) {
                // given
                let group = super::manage_groups::Group::new(
                    "group",
                    Some([(connector.config.source_room_name_attr.clone(), vec!["group_name".to_owned()])].into()),
                );
                let synapse_room = MockSynapseRoomBuilder::default().source_room_id(group.id()).build();
                let user = User::new("user", None, None, None, None, true, None, Some(vec![group]), None);
                let user_id = user.id;
                let synapse_user = MockSynapseUserBuilder::default().source_user_id(user_id.clone()).build();
                {
                    // 1. Create user.
                    connector
                        .replace_api_mock(
                            SynapseApiMocker::new(SYNCER_USER_ID)
                                .with_rooms(vec![synapse_room])
                                .with_users(vec![synapse_user.clone()])
                                .can_get_joined_rooms_of_syncer()
                                .can_get_users()
                                .can_get_source_user_id_for_all_matrix_users()
                                .can_get_room_associated_source_group_id_v1()
                                .can_associate_source_group_id_to_room()
                                .can_get_all_rooms_associated_source_group_id(),
                        )
                        .await;
                    let all_users = connector.all_users().await.unwrap();
                    assert!(all_users.contains(&user_id));
                }
                {
                    // 2. Delete user.
                    connector.synapse_interactor = SynapseApiMocker::new(SYNCER_USER_ID).require_deactivate_user(&synapse_user).into();

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
