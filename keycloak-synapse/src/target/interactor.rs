pub struct SynapseInteractor {
    synapse_api: Box<dyn crate::target::external::SynapseApi + Send + Sync>,
}

impl SynapseInteractor {
    pub fn new(synapse_api: impl crate::target::external::SynapseApi + Send + Sync + 'static) -> Self {
        Self {
            synapse_api: Box::new(synapse_api),
        }
    }

    pub fn synapse_api(&self) -> &(dyn crate::target::external::SynapseApi + Send + Sync) {
        self.synapse_api.as_ref()
    }

    /// The old syncer used a different event to associate matrix rooms to keycloak rooms.
    /// This function migrates rooms to the new format.
    /// Once the new syncer was successfully run once, we should be able to delete this method.
    pub async fn migrate(&self, rooms: &[String]) -> Result<(), kids_lib::error::KidsError> {
        for room in rooms {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            if let Ok(source_id) = self.synapse_api.get_room_associated_source_group_id_v1(room).await {
                match self.synapse_api.associate_source_group_id_to_room(room, &source_id).await {
                    Ok(()) => tracing::info!(room, "Migrated room"),
                    Err(e) => {
                        tracing::error!(?e, room, "Failed to migrate room");
                        return Err(e);
                    }
                };
            }
        }
        Ok(())
    }

    pub fn generate_matrix_user_id(&self, username: &str) -> crate::target::types::MatrixUserId {
        crate::target::types::MatrixUserId {
            username: username.to_owned(),
            homeserver: self.synapse_api.homeserver_domain().clone(),
        }
    }

    pub async fn get_user_from_mas_user(
        &self,
        mas_user: crate::target::dto::mas::UserResponse,
    ) -> Result<crate::target::types::User, kids_lib::error::KidsError> {
        let matrix_user_id = self.generate_matrix_user_id(mas_user.attributes.username.as_str());
        let source_user_id = self.synapse_api.get_source_user_id_for_mas_user_id(&mas_user.id).await?;
        let deactivated = mas_user.attributes.deactivated_at.is_some();
        let display_name = self.synapse_api.get_user_display_name(&matrix_user_id).await.unwrap_or(
            // Getting the display name might fail e.g. when the user is deactivated.
            // In this case, we assume no set display name which will be fixed by the sync later on.
            None,
        );
        let emails = self.synapse_api.get_user_emails(&mas_user.id).await?;
        let rooms = self.synapse_api.get_user_joined_rooms(&matrix_user_id).await?.joined_rooms;
        let user = crate::target::types::User {
            matrix_user_id,
            mas_user_id: mas_user.id,
            source_user_id,
            deactivated,
            state: crate::target::types::UserState {
                display_name,
                emails,
                locked: mas_user.attributes.locked_at.is_some(),
                is_admin: mas_user.attributes.admin,
                rooms,
            },
        };
        Ok(user)
    }

    pub async fn get_users(&self) -> Result<Vec<crate::target::types::User>, kids_lib::error::KidsError> {
        let mas_users = self.synapse_api.get_mas_users().await?;
        let mut users = Vec::with_capacity(mas_users.len());
        for mas_user in mas_users {
            let user = self.get_user_from_mas_user(mas_user).await?;
            users.push(user);
        }
        Ok(users)
    }

    pub async fn create_user(
        &self,
        matrix_user_id: &crate::target::types::MatrixUserId,
        source_user_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<crate::target::types::User, kids_lib::error::KidsError> {
        let mas_user = self.synapse_api.create_user(matrix_user_id, source_user_id).await?;
        self.get_user_from_mas_user(mas_user).await
    }

    pub async fn ensure_group_display_name(&self, matrix_room_id: &str, desired_name: String) {
        let old_display_name = self.synapse_api.get_room_display_name(matrix_room_id).await;
        match old_display_name {
            Ok(old_display_name) if old_display_name != desired_name => match self.synapse_api.set_room_display_name(matrix_room_id, &desired_name).await {
                Ok(()) => tracing::debug!(old_display_name, desired_name, matrix_room_id, "Updated display name of room"),
                Err(e) => tracing::warn!(?e, matrix_room_id, "Could not update display name of room"),
            },
            Ok(old_display_name) => {
                tracing::trace!(matrix_room_id, display_name = old_display_name, "Keeping existing room display name")
            }
            Err(e) => tracing::warn!(?e, matrix_room_id, "Could not load display name of room"),
        }
    }

    pub async fn ensure_group_canonical_alias(&self, matrix_room_id: &str, desired_alias: String) {
        let canonical_alias_event = self.synapse_api.get_room_canonical_alias(matrix_room_id).await;
        match canonical_alias_event {
            Ok(canonical_alias_event) => {
                tracing::trace!(?canonical_alias_event, matrix_room_id, "Found canonical alias for room");

                if canonical_alias_event.alias == desired_alias {
                    return;
                }

                match self.synapse_api.create_room_alias(matrix_room_id, &desired_alias).await {
                    Ok(()) => tracing::debug!(matrix_room_id, desired_alias, "Created new room alias"),
                    Err(e) => {
                        tracing::warn!(
                            ?e,
                            matrix_room_id,
                            ?desired_alias,
                            "Could not create new alias for room. Aborting update of canonical alias"
                        );
                        return;
                    }
                }

                match self.synapse_api.set_room_canonical_alias(matrix_room_id, &desired_alias).await {
                    Ok(()) => tracing::debug!(matrix_room_id, desired_alias, "Updated canonical alias for room"),
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

    pub async fn delete_room(
        &self,
        matrix_room_id: &str,
        room_deletion_strategy: crate::target::RoomDeletionStrategy,
    ) -> Result<(), kids_lib::error::KidsError> {
        match room_deletion_strategy {
            crate::target::RoomDeletionStrategy::KickAll | crate::target::RoomDeletionStrategy::Evacuate => {
                let joined_members = self
                    .synapse_api
                    .get_room_joined_users(matrix_room_id)
                    .await
                    .map_err(|e| e.with_context("Could not get joined users for room deletion. Could not delete room"))?;
                tracing::info!(matrix_room_id, ?joined_members, "Kicking members from room");

                let mut all_kicked = true;
                for member in joined_members.joined.keys() {
                    tracing::debug!(matrix_room_id, member = member.display(), "Kicking member from room");
                    if self.synapse_api.user_is_matrix_syncer(member) {
                        continue;
                    }
                    if let Err(e) = self.synapse_api.kick_user_from_room(matrix_room_id, member).await {
                        tracing::error!(matrix_room_id, member = member.display(), error = ?e, "Could not kick member from room");
                        all_kicked = false;
                    }
                }

                if !all_kicked {
                    // Note: Need to return early here because the syncer should only leave the room
                    // if all users have been kicked successfully.
                    return Err(kids_lib::error::KidsError::InternalError(anyhow::anyhow!(
                        "Could not kick all members from room {matrix_room_id}"
                    )));
                }

                if matches!(room_deletion_strategy, crate::target::RoomDeletionStrategy::Evacuate) {
                    tracing::info!(matrix_room_id, "Syncer leaving room");
                    self.synapse_api
                        .syncer_leave_room(matrix_room_id)
                        .await
                        .map_err(|e| e.with_context("Could not leave room"))?;
                }
            }
            crate::target::RoomDeletionStrategy::Delete => {
                tracing::info!(matrix_room_id, "Deleting room");
                self.synapse_api
                    .delete_room(matrix_room_id)
                    .await
                    .map_err(|e| e.with_context("Could not delete room"))?;
            }
            crate::target::RoomDeletionStrategy::Ignore => {}
        }

        // Note: If strategy is delete, alias has already been deleted with the room.
        if matches!(room_deletion_strategy, crate::target::RoomDeletionStrategy::Evacuate) {
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
