pub struct IdMapping {
    group_id_mapping: std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, String>,
    user_id_mapping: std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, crate::target::dto::User>,
    /// The source id of the syncer user, if present.
    /// This user will be ignored.
    syncer_source_user_id: Option<kids_lib::types::SharedResourceIdentifier>,
}

impl IdMapping {
    pub const fn get_group_id_mapping(&self) -> &std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, String> {
        &self.group_id_mapping
    }
    pub const fn get_user_id_mapping(&self) -> &std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, crate::target::dto::User> {
        &self.user_id_mapping
    }
    pub const fn get_group_id_mapping_mut(&mut self) -> &mut std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, String> {
        &mut self.group_id_mapping
    }
    pub const fn get_user_id_mapping_mut(&mut self) -> &mut std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, crate::target::dto::User> {
        &mut self.user_id_mapping
    }
    pub const fn get_syncer_source_user_id(&self) -> Option<&kids_lib::types::SharedResourceIdentifier> {
        self.syncer_source_user_id.as_ref()
    }

    pub fn has_user(&self, source_user_id: &str) -> bool {
        self.get_user_opt(source_user_id).is_some()
    }

    pub fn get_user_opt(&self, source_user_id: &str) -> Option<&crate::target::dto::User> {
        self.get_user_id_mapping().get(source_user_id)
    }

    pub fn get_user_opt_mut(&mut self, source_user_id: &str) -> Option<&mut crate::target::dto::User> {
        self.get_user_id_mapping_mut().get_mut(source_user_id)
    }

    /// This method panics when the user cannot be found.
    pub fn get_user(&self, source_user_id: &str) -> &crate::target::dto::User {
        self.get_user_opt(source_user_id)
            .expect("User not found, although it should be guaranteed it exists")
    }

    /// This method panics when the user cannot be found.
    pub fn get_user_mut(&mut self, source_user_id: &str) -> &mut crate::target::dto::User {
        self.get_user_opt_mut(source_user_id)
            .expect("User not found, although it should be guaranteed it exists")
    }

    pub fn has_group(&self, source_group_id: &str) -> bool {
        self.get_group_opt(source_group_id).is_some()
    }

    pub fn get_group_opt(&self, source_group_id: &str) -> Option<&String> {
        self.get_group_id_mapping().get(source_group_id)
    }

    pub fn get_group_opt_mut(&mut self, source_group_id: &str) -> Option<&mut String> {
        self.get_group_id_mapping_mut().get_mut(source_group_id)
    }

    /// This method panics when the group cannot be found.
    pub fn get_group(&self, source_group_id: &str) -> &String {
        self.get_group_opt(source_group_id)
            .expect("Group not found, although it should be guaranteed it exists")
    }

    async fn generate_group_id_mapping(
        synapse_interactor: &crate::target::SynapseInteractor,
    ) -> Result<std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, String>, kids_lib::error::KidsError> {
        let matrix_syncer_joined_rooms = synapse_interactor
            .synapse_api()
            .get_joined_rooms_of_syncer()
            .await
            .map_err(|e| e.with_context("Failed getting rooms syncer has joined"))?
            .joined_rooms;

        synapse_interactor.migrate(&matrix_syncer_joined_rooms).await;

        let mut group_id_mapping = std::collections::HashMap::new();

        for matrix_room_id in matrix_syncer_joined_rooms {
            // If this request fails, we don't want to abort the whole method as it only affects a single room.
            // Note that in this case, we might actually create a second room for the same group (if there exists a room mapped to the group
            // in Synapse already, but we failed to obtain that mapping).
            let source_group_id = match synapse_interactor.synapse_api().get_room_associated_source_group_id(&matrix_room_id).await {
                Ok(source_group_id) => source_group_id,
                Err(error) => {
                    // To reach eventual consistency, we need to delete all rooms the syncer is in
                    // that have no source groups associated to them.
                    // This is because such a situation might happen when creation of a room succeeds, but
                    // the subsequent request to associate the source ID fails. In that case, the syncer would retry
                    // creating a room for the associated group, but that would fail because the desired alias
                    // of that room would clash with the one previously created.
                    if let kids_lib::error::KidsError::ApiOperationFailed(_, 404, ..) = error {
                        tracing::error!(
                            matrix_room_id,
                            "Encountered a room the syncer has joined that has no source group associated to it. Deleting that room"
                        );
                        if let Err(e) = synapse_interactor
                            .delete_room(&matrix_room_id, crate::target::RoomDeletionStrategy::Delete)
                            .await
                        {
                            tracing::warn!(matrix_room_id, error=%e, "Could not delete room with no associated source group id");
                        }
                    } else {
                        tracing::error!(?error, matrix_room_id, "Could not determine source group for room");
                    }

                    continue;
                }
            };
            if group_id_mapping.contains_key(&source_group_id) {
                tracing::warn!(
                    source_group_id,
                    first_room_id = matrix_room_id,
                    second_room_id = group_id_mapping[&source_group_id],
                    "Found duplicate mapping for source group"
                );
                // We don't really know which room really is the better one to use in case of duplicate mapping,
                // so might as well go with the first one we already encountered earlier.
                continue;
            }

            group_id_mapping.insert(source_group_id, matrix_room_id);
        }
        Ok(group_id_mapping)
    }

    async fn generate_user_id_mapping(
        synapse_interactor: &crate::target::SynapseInteractor,
    ) -> Result<
        (
            std::collections::HashMap<kids_lib::types::SharedResourceIdentifier, crate::target::dto::User>,
            Option<kids_lib::types::SharedResourceIdentifier>,
        ),
        kids_lib::error::KidsError,
    > {
        let matrix_users = synapse_interactor
            .synapse_api()
            .get_users()
            .await
            .map_err(|e| e.with_context("Failed getting matrix users"))?;

        let mut syncer_source_user_id = None;
        let mut user_id_mapping: std::collections::HashMap<String, crate::target::dto::User> = std::collections::HashMap::new();
        for user in matrix_users.users {
            let source_user_id = synapse_interactor.synapse_api().get_source_user_id_for_matrix_user_id(&user.name).await;
            let is_syncer_user = synapse_interactor.synapse_api().user_is_matrix_syncer(user.name.as_str());
            let source_user_id = match (source_user_id, is_syncer_user) {
                (Ok(source_user_id), false) => source_user_id,
                (Ok(source_user_id), true) => {
                    syncer_source_user_id = Some(source_user_id);
                    continue;
                }
                (Err(error), false) => {
                    tracing::warn!(%error, matrix_user_id=user.name, "Could not obtain source user ID for matrix user");
                    continue;
                }
                (Err(error), true) => {
                    tracing::trace!(%error, matrix_user_id=user.name, "Could not obtain source user ID for syncer user");
                    continue;
                }
            };

            if user_id_mapping.contains_key(&source_user_id) {
                // This should not happen because matrix does not allow creating two users with equal source ids
                // (otherwise, when logging in via SSO, matrix would not know which user to login).
                tracing::error!(
                    source_user_id,
                    first_matrix_user_id = user.name,
                    second_matrix_user_id = user_id_mapping[&source_user_id].name,
                    "Found duplicate mapping for source user"
                );
                continue;
            }

            user_id_mapping.insert(source_user_id, user);
        }
        Ok((user_id_mapping, syncer_source_user_id))
    }

    /// This function generates different id mappings required as user and group ids in
    /// Keycloak are different than in Synapse.
    ///
    /// Therefore, we need a mapping between both, which is built in this function.
    pub async fn generate(synapse_interactor: &crate::target::SynapseInteractor) -> Result<Self, kids_lib::error::KidsError> {
        let group_id_mapping = Self::generate_group_id_mapping(synapse_interactor).await?;

        let (user_id_mapping, syncer_source_user_id) = Self::generate_user_id_mapping(synapse_interactor).await?;

        Ok(Self {
            group_id_mapping,
            syncer_source_user_id,
            user_id_mapping,
        })
    }
}

#[cfg(test)]
impl IdMapping {
    pub fn empty() -> Self {
        Self {
            group_id_mapping: std::collections::HashMap::new(),
            user_id_mapping: std::collections::HashMap::new(),
            syncer_source_user_id: None,
        }
    }
}
