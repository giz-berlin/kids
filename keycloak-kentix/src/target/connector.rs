#[derive(serde::Deserialize)]
pub struct KentixConfig {
    kentix_api: super::external::KentixApiConfig,
    /// Users with this role set will have `emergency_access` enabled.
    offline_access_role: String,
    /// The source user attribute name (key) for the [RFID UID](super::dto::UserRfidUid), as a hex string.
    rfid_uid_attribute_name: String,
    /// The source user attribute name (key) for the [RFID Data](super::dto::UserRfidData).
    rfid_data_attribute_name: String,
}

pub struct Connector {
    config: KentixConfig,
    kentix_api: Box<dyn super::external::KentixApi + Send + Sync>,
    users: crate::target::UserIdMapping,
}

impl Connector {
    /// Get all [Levelprofiles/Access Profiles](super::dto::Levelprofile) known to Kentix, indexed by their [name](super::dto::LevelprofileName).
    ///
    /// This is *always* an eager load as we will never be informed by anyone if the levelprofiles change inside of Kentix.
    async fn get_levelprofiles(
        kentix_api: &(dyn super::external::KentixApi + Send + Sync),
    ) -> Result<std::collections::HashMap<super::dto::LevelprofileName, super::dto::Levelprofile>, kids_lib::error::KidsError> {
        let levelprofiles = kentix_api.get_levelprofiles().await?;
        let levelprofiles = levelprofiles.into_iter().map(|profile| (profile.name.clone(), profile)).collect();
        Ok(levelprofiles)
    }
}

#[async_trait::async_trait]
impl kids_lib::interface::target::Target for Connector {
    type Config = KentixConfig;

    async fn new(config: Self::Config) -> Result<Self, kids_lib::error::KidsError> {
        tracing::trace!("Creating new Kentix Connector");
        let kentix_api = Box::new(super::external::KentixClient::new(config.kentix_api.clone()));
        let users = crate::target::UserIdMapping::generate(kentix_api.as_ref()).await?;
        Ok(Self { config, kentix_api, users })
    }

    fn info(&self) -> String {
        "Kentix Connector!".to_owned()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), kids_lib::error::KidsError> {
        tracing::trace!("Notification of full sync");
        self.users = crate::target::UserIdMapping::generate(self.kentix_api.as_ref()).await?;
        Ok(())
    }

    async fn all_groups(&mut self) -> Result<std::collections::HashSet<kids_lib::types::SharedResourceIdentifier>, kids_lib::error::KidsError> {
        tracing::trace!("Getting all groups");
        Ok(Default::default())
    }

    async fn all_users(&mut self) -> Result<std::collections::HashSet<kids_lib::types::SharedResourceIdentifier>, kids_lib::error::KidsError> {
        tracing::trace!("Getting all users");
        Ok(self.users.users().values().map(|user| user.user.username.0.clone()).collect())
    }

    async fn delete_group(&mut self, group_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), kids_lib::error::KidsError> {
        tracing::trace!(source_group_id = group_id, "Deleting group");
        tracing::warn!(
            source_group_id = group_id,
            "The group has been deleted. All users who gained levelprofiles from this group retain them until the next full-sync occurs."
        );
        Ok(())
    }

    async fn delete_user(&mut self, user_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), kids_lib::error::KidsError> {
        tracing::trace!(source_user_id = user_id, "Deleting user");
        let kentix_username = super::dto::Username(user_id.clone());
        let existing_users = self.users.users_mut();
        let existing_kentix_user = existing_users.remove(&kentix_username);
        match existing_kentix_user {
            Some(existing_kentix_user) => {
                if let Err(err_box) = self.kentix_api.delete_user(existing_kentix_user).await {
                    let (user, err) = *err_box;
                    existing_users.insert(kentix_username, user);
                    return Err(err);
                }
                tracing::debug!(source_user_id = user_id, "Deleted user");
                Ok(())
            }
            None => {
                const ERROR_CONTEXT: &str = "Cannot delete user";
                const ERROR_MSG: &str = "The user does not exist.";
                tracing::error!(source_user_id = %user_id, "{ERROR_CONTEXT}: {ERROR_MSG}");
                return Err(kids_lib::error::KidsError::RequestFailed(
                    ERROR_CONTEXT.to_owned(),
                    anyhow::anyhow!("{ERROR_MSG}"),
                ));
            }
        }
    }

    async fn create_or_update_group(
        &mut self,
        group: std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>,
    ) -> Result<(), kids_lib::error::KidsError> {
        tracing::trace!(source_group_id = group.id(), "Creating or updating group");
        // Update all users in this group.
        // If the update was for a role change, we need to update the users to reflect the new levelprofiles.
        for user in group.users(true).await? {
            self.create_or_update_user(user).await?;
        }
        Ok(())
    }

    async fn create_or_update_user(
        &mut self,
        source_user: std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>,
    ) -> Result<(), kids_lib::error::KidsError> {
        tracing::trace!(source_user_id = source_user.id(), "Creating or updating user");
        let kentix_username = super::dto::Username(source_user.id().clone());
        let existing_users = self.users.users_mut();
        let existing_kentix_user = existing_users.get_mut(&kentix_username);
        let (rfid_uid, rfid_data) = {
            let rfid_uid = source_user.attributes().get(&self.config.rfid_uid_attribute_name);
            let rfid_data = source_user.attributes().get(&self.config.rfid_data_attribute_name);
            match (rfid_uid.map(Vec::as_slice), rfid_data.map(Vec::as_slice), existing_kentix_user.as_ref()) {
                (Some([rfid_uid]), Some([rfid_data]), _) => (
                    rfid_uid.try_into().map_err(|err| {
                        const ERROR_CONTEXT: &str = "Cannot add or create user";
                        const ERROR_MSG: &str = "Found invalid RFID UID";
                        tracing::error!(source_user_id = %source_user.id(), rfid_uid, "{ERROR_CONTEXT}: {ERROR_MSG}: {err}");
                        kids_lib::error::KidsError::RequestFailed(ERROR_CONTEXT.to_owned(), anyhow::anyhow!("{ERROR_MSG}"))
                    })?,
                    super::dto::UserRfidData(rfid_data.clone()),
                ),
                (Some(_), Some(_), _) => {
                    const ERROR_CONTEXT: &str = "Cannot add or create user";
                    const ERROR_MSG: &str = "Found multiple RFID UID or data.";
                    tracing::error!(source_user_id = %source_user.id(), "{ERROR_CONTEXT}: {ERROR_MSG}");
                    return Err(kids_lib::error::KidsError::RequestFailed(
                        ERROR_CONTEXT.to_owned(),
                        anyhow::anyhow!("{ERROR_MSG}"),
                    ));
                }
                // Kentix does support user accounts without RFID UID/data.
                // We disallow them in synced users, though, as all users we sync are synced for the purpose of using the locks.
                // Accounts without a key, e.g. management accounts, are not managed via KIDS (see `ignored_usernames` of `KentixApiConfig` in this context).
                (_, _, Some(_)) => {
                    tracing::debug!(
                        source_user_id = source_user.id(),
                        "User no longer has the required RFID UID/data attached. Deleting them."
                    );
                    return self.delete_user(source_user.id()).await;
                }
                (_, _, None) => {
                    tracing::trace!(source_user_id = source_user.id(), "Skipping user without required RFID UID/data attached.");
                    return Ok(());
                }
            }
        };
        // Only do this after verifying the user is eligible for an account
        // via the RFID UID/data above.
        // This way, we only issue the expensive call for levelprofiles when necessary.
        let levelprofiles = Self::get_levelprofiles(self.kentix_api.as_ref()).await?;
        let roles = source_user.roles().await?;
        let offline_access = roles.contains(&self.config.offline_access_role);
        let desired_levelprofiles = roles
            .into_iter()
            // This role is special and does not correspond to a levelprofile.
            .filter(|role| *role != self.config.offline_access_role)
            .map(super::dto::LevelprofileName)
            .collect::<Vec<_>>();
        tracing::trace!(source_user_id = source_user.id(), desired_levelprofiles = ?desired_levelprofiles);
        let desired_levelprofiles = match desired_levelprofiles
            .into_iter()
            .map(|profile_name| levelprofiles.get(&profile_name))
            .collect::<Option<Vec<_>>>()
        {
            Some(profiles) => profiles,
            None => {
                const ERROR_CONTEXT: &str = "Cannot add or create user";
                const ERROR_MSG: &str = "Not all desired levelprofiles exist in Kentix.";
                tracing::error!(source_user_id = %source_user.id(), "{ERROR_CONTEXT}: {ERROR_MSG}");
                return Err(kids_lib::error::KidsError::RequestFailed(
                    ERROR_CONTEXT.to_owned(),
                    anyhow::anyhow!("{ERROR_MSG}"),
                ));
            }
        };

        let new_kentix_user = {
            let source_username = match source_user.username() {
                Some(username) => username,
                None => {
                    const ERROR_CONTEXT: &str = "Cannot add or create user";
                    const ERROR_MSG: &str = "The user has no username.";
                    tracing::error!(source_user_id = %source_user.id(), "{ERROR_CONTEXT}: {ERROR_MSG}");
                    return Err(kids_lib::error::KidsError::RequestFailed(
                        ERROR_CONTEXT.to_owned(),
                        anyhow::anyhow!("{ERROR_MSG}"),
                    ));
                }
            };
            super::dto::User {
                username: kentix_username,
                fullname: super::dto::UserFullName(source_username.to_owned()),
                email: source_user.email().map(|email| super::dto::UserEmail(email.to_owned())),
                is_active: super::dto::UserActive(source_user.enabled()),
                emergency_access: super::dto::UserEmergencyAccess(offline_access),
                levelprofiles: desired_levelprofiles.into_iter().map(|profile| profile.id).collect(),
                rfid_uid: Some(rfid_uid),
                rfid_data: Some(rfid_data),
            }
        };
        match existing_kentix_user {
            Some(existing_kentix_user) => {
                if existing_kentix_user.user == new_kentix_user {
                    tracing::trace!(source_user_id = source_user.id(), "No update to user required; there are no changes.");
                    return Ok(());
                } else {
                    tracing::debug!(existing_user = ?existing_kentix_user.user, new_user = ?new_kentix_user, "Updating user");
                }
                let kentix_user_with_id = super::dto::UserWithId {
                    id: existing_kentix_user.id,
                    user: new_kentix_user,
                };
                let kentix_user = self.kentix_api.update_user(kentix_user_with_id).await?;
                existing_kentix_user.user = kentix_user.user;
                Ok(())
            }
            None => {
                tracing::debug!(new_user = ?new_kentix_user, "Creating user");
                let kentix_user = self.kentix_api.create_user(new_kentix_user).await?;
                existing_users.insert(kentix_user.user.username.clone(), kentix_user);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::target::test_mocks::KentixApiMocker;
    use kids_lib::interface::target::Target;

    const OFFLINE_ACCESS_ROLE: &str = "offline-access";
    const RFID_UID_ATTRIBUTE_NAME: &str = "kentix-uid";
    const RFID_DATA_ATTRIBUTE_NAME: &str = "kentix-data";

    #[rstest::fixture]
    pub fn connector() -> Connector {
        Connector {
            config: KentixConfig {
                kentix_api: crate::target::external::KentixApiConfig {
                    ignored_usernames: vec!["admin".into()],
                    bearer_token: String::new(),
                    insecure_disable_tls_verification: true,
                    kentix_root_certificate_pem_path: None,
                    kentix_url: url::Url::parse("http://localhost").unwrap(),
                },
                offline_access_role: OFFLINE_ACCESS_ROLE.to_owned(),
                rfid_uid_attribute_name: RFID_UID_ATTRIBUTE_NAME.to_owned(),
                rfid_data_attribute_name: RFID_DATA_ATTRIBUTE_NAME.to_owned(),
            },
            kentix_api: KentixApiMocker::default().into(),
            users: crate::target::UserIdMapping::empty(),
        }
    }

    impl Connector {
        /// Replaces the API mock **and** performs a full sync.
        ///
        /// This allows you to refer to new users not added via the [Connector] itself.
        async fn replace_api_mock(&mut self, kentix_api: crate::target::test_mocks::KentixApiMocker) {
            self.kentix_api = kentix_api.into();
            self.full_sync_incoming().await.expect("full_sync_incoming should not fail");
        }
    }

    #[rstest::rstest]
    fn info_works(connector: Connector) {
        assert_eq!(connector.info(), "Kentix Connector!")
    }

    mod when_full_sync_incoming {
        use super::*;

        #[rstest::rstest]
        #[tokio::test]
        async fn then_return_ok(mut connector: Connector) {
            // given
            connector.kentix_api = KentixApiMocker::default().can_get_all_users().into();

            // when
            let full_sync_incoming_result = connector.full_sync_incoming().await;

            // then
            assert!(matches!(full_sync_incoming_result, Ok(())));
        }

        #[rstest::rstest]
        #[tokio::test]
        async fn then_add_users_to_user_mapping(mut connector: Connector) {
            // given
            let user1 = crate::target::dto::UserWithId::builder()
                .id(1)
                .user(
                    crate::target::dto::User::builder()
                        .username(kids_test_lib::util::constants::DEFAULT_SOURCE_USER_ID)
                        .fullname("user1.name")
                        .is_active(true)
                        .emergency_access(false)
                        .rfid_uid(123)
                        .rfid_data("456")
                        .build(),
                )
                .build();
            let user2 = crate::target::dto::UserWithId::builder()
                .id(2)
                .user(
                    crate::target::dto::User::builder()
                        .username(kids_test_lib::util::constants::ANOTHER_SOURCE_USER_ID)
                        .fullname("user2.name")
                        .email("user2@example.com")
                        .is_active(false)
                        .emergency_access(false)
                        .rfid_uid(123)
                        .rfid_data("456")
                        .build(),
                )
                .build();

            connector.kentix_api = KentixApiMocker::default().with_users([user1.clone(), user2.clone()]).can_get_all_users().into();

            // when
            let full_sync_result = connector.full_sync_incoming().await;

            // then
            assert!(matches!(full_sync_result, Ok(())));
            let user_mapping = connector.users.users();
            assert_eq!(user_mapping.len(), 2);
            assert_eq!(*user_mapping.get(&user1.user.username).unwrap(), user1);
            assert_eq!(*user_mapping.get(&user2.user.username).unwrap(), user2);
        }

        #[rstest::rstest]
        #[tokio::test]
        async fn then_completely_clears_mapping(mut connector: Connector) {
            // given
            let user = crate::target::dto::UserWithId::builder()
                .id(0)
                .user(
                    crate::target::dto::User::builder()
                        .username(kids_test_lib::util::constants::DEFAULT_SOURCE_USER_ID)
                        .fullname("user.name")
                        .is_active(true)
                        .emergency_access(false)
                        .rfid_uid(123)
                        .rfid_data("456")
                        .build(),
                )
                .build();
            connector.kentix_api = KentixApiMocker::default().can_get_all_users().into();
            connector.users.users_mut().insert(user.user.username.clone(), user);

            // when
            let full_sync_incoming_result = connector.full_sync_incoming().await;

            // then
            assert!(matches!(full_sync_incoming_result, Ok(())));
            assert!(connector.users.users().is_empty());
        }

        #[rstest::rstest]
        #[tokio::test]
        async fn but_cannot_get_users_then_return_err(mut connector: Connector) {
            // given
            connector.kentix_api = KentixApiMocker::default().errors_get_all_users().into();

            // when
            let full_sync_incoming_result = connector.full_sync_incoming().await;

            // then
            assert!(matches!(
                full_sync_incoming_result,
                Err(kids_lib::error::KidsError::InternalError(err))
                if err == crate::target::test_mocks::EXPLICITLY_FORBIDDEN_METHOD
            ));
        }
    }

    mod manage_groups {
        use super::*;

        mod create_and_update {
            use super::*;

            #[rstest::rstest]
            #[tokio::test]
            async fn does_nothing(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new("1", None);

                // when
                let create_or_add_group_result = connector.create_or_update_group(std::sync::Arc::new(group)).await;

                // then
                assert!(matches!(create_or_add_group_result, Ok(())));
                let all_groups = connector.all_groups().await.unwrap();
                assert!(all_groups.is_empty());
            }
        }

        mod delete {
            use super::*;

            #[rstest::rstest]
            #[tokio::test]
            async fn does_nothing(mut connector: Connector) {
                // given
                let group = kids_test_lib::Group::new("1", None);
                let all_groups = connector.all_groups().await.unwrap();
                assert!(all_groups.is_empty(), "Sanity: No groups exist.");

                // when
                let create_or_add_group_result = connector.delete_group(&group.id).await;

                // then
                assert!(matches!(create_or_add_group_result, Ok(())));
                let all_groups = connector.all_groups().await.unwrap();
                assert!(all_groups.is_empty());
            }
        }
    }

    mod manage_users {
        use super::*;

        mod create {
            use super::*;

            #[rstest::rstest]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                .id("my-sub")
                .username("firstname.lastname")
                .first_name("Firstname")
                .last_name("Lastname")
                .enabled(true)
                .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                .build(),
            crate::target::dto::UserWithId::builder().id(2).user(crate::target::dto::User::builder()
                .username("my-sub")
                .fullname("firstname.lastname")
                .is_active(true)
                .emergency_access(false)
                .rfid_uid(123)
                .rfid_data("456")
                .build()).build(),
            [])]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                .id("my-sub")
                .username("firstname.lastname")
                .first_name("Firstname")
                .last_name("Lastname")
                .enabled(false)
                .email("user@example.com")
                .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                .build(),
            crate::target::dto::UserWithId::builder().id(3).user(crate::target::dto::User::builder()
                .username("my-sub")
                .fullname("firstname.lastname")
                .email("user@example.com")
                .is_active(false)
                .emergency_access(false)
                .rfid_uid(123)
                .rfid_data("456")
                .build()).build(),
            [])]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                .id("my-sub")
                .username("firstname.lastname")
                .first_name("Firstname")
                .last_name("Lastname")
                .enabled(true)
                .with_role(OFFLINE_ACCESS_ROLE)
                .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                .build(),
            crate::target::dto::UserWithId::builder().id(5).user(crate::target::dto::User::builder()
                .username("my-sub")
                .fullname("firstname.lastname")
                .is_active(true)
                .emergency_access(true)
                .rfid_uid(123)
                .rfid_data("456")
                .build()).build(),
            [crate::target::dto::Levelprofile::builder().id(7).name("main-entrance").build()])]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                .id("my-sub")
                .username("firstname.lastname")
                .enabled(true)
                .with_role("main-entrance")
                .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                .build(),
            crate::target::dto::UserWithId::builder().id(11).user(crate::target::dto::User::builder()
                .username("my-sub")
                .fullname("firstname.lastname")
                .is_active(true)
                .with_levelprofile(13)
                .emergency_access(false)
                .rfid_uid(123)
                .rfid_data("456")
                .build()).build(),
            [crate::target::dto::Levelprofile::builder().id(13).name("main-entrance").build()])]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                .id("my-sub")
                .username("firstname.lastname")
                .enabled(true)
                .with_role("main-entrance")
                .with_role("server-room")
                .with_role(OFFLINE_ACCESS_ROLE)
                .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                .build(),
            crate::target::dto::UserWithId::builder().id(17).user(crate::target::dto::User::builder()
                .username("my-sub")
                .fullname("firstname.lastname")
                .is_active(true)
                .with_levelprofile(19)
                .with_levelprofile(23)
                .emergency_access(true)
                .rfid_uid(123)
                .rfid_data("456")
                .build()).build(),
            [
                crate::target::dto::Levelprofile::builder().id(19).name("main-entrance").build(),
                crate::target::dto::Levelprofile::builder().id(23).name("server-room").build(),
                crate::target::dto::Levelprofile::builder().id(29).name("cellar").build(),
            ])]
            // Test that the `OFFLINE_ACCESS_ROLE` never grants a levelprofile.
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                .id("my-sub")
                .username("firstname.lastname")
                .enabled(true)
                .with_role("main-entrance")
                .with_role(OFFLINE_ACCESS_ROLE)
                .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                .build(),
            crate::target::dto::UserWithId::builder().id(31).user(crate::target::dto::User::builder()
                .username("my-sub")
                .fullname("firstname.lastname")
                .is_active(true)
                .with_levelprofile(37)
                .emergency_access(true)
                .rfid_uid(123)
                .rfid_data("456")
                .build()).build(),
            [
                crate::target::dto::Levelprofile::builder().id(37).name("main-entrance").build(),
                crate::target::dto::Levelprofile::builder().id(41).name(OFFLINE_ACCESS_ROLE).build(),
            ])]
            async fn create_user_succeeds(
                mut connector: Connector,
                #[case] source_user: impl kids_lib::interface::source::User + Send + Sync + 'static,
                #[case] expected_kentix_user: crate::target::dto::UserWithId,
                #[case] available_levelprofiles: impl Into<Vec<crate::target::dto::Levelprofile>>,
            ) {
                // given
                connector
                    .replace_api_mock(
                        KentixApiMocker::default()
                            .with_levelprofiles(available_levelprofiles)
                            .can_get_all_levelprofiles()
                            .can_get_all_users()
                            .require_create_user(expected_kentix_user.user.clone(), expected_kentix_user.id),
                    )
                    .await;
                assert!(connector.all_users().await.unwrap().is_empty(), "Sanity: The mapping is empty beforehand.");

                // when
                let created_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                // then
                created_result.expect("Error creating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.len() == 1);
                let created_user_id = all_users.iter().next().unwrap();
                assert_eq!(created_user_id, &expected_kentix_user.user.username.0);
                let (created_user_id, created_user) = connector.users.users().iter().next().unwrap();
                assert_eq!(*created_user_id, expected_kentix_user.user.username);
                assert_eq!(created_user, &expected_kentix_user);
            }

            #[rstest::rstest]
            #[tokio::test]
            async fn create_user_noop_without_rfid_uid_or_data(mut connector: Connector) {
                // given
                let get_source_user_builder = || {
                    kids_test_lib::User::builder()
                        .id("my-sub")
                        .username("firstname.lastname")
                        .enabled(true)
                        .with_role("main-entrance")
                };
                enum Set {
                    None,
                    Uid,
                    Data, /* setting both is handled above in the "normal" case */
                }
                for set in [Set::None, Set::Uid, Set::Data] {
                    let source_user = match set {
                        Set::None => get_source_user_builder().build(),
                        Set::Uid => get_source_user_builder().with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b").build(),
                        Set::Data => get_source_user_builder().with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456").build(),
                    };
                    connector
                        .replace_api_mock(KentixApiMocker::default().can_get_all_levelprofiles().can_get_all_users())
                        .await;
                    assert!(connector.all_users().await.unwrap().is_empty(), "Sanity: The mapping is empty beforehand.");

                    // when
                    let created_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                    // then
                    created_result.expect("The operation should be successful...");
                    assert!(connector.all_users().await.unwrap().is_empty(), "...but the mapping should still be empty.");
                }
            }

            #[rstest::rstest]
            #[tokio::test]
            async fn create_user_fails_without_levelprofile(mut connector: Connector) {
                // given
                let source_user = kids_test_lib::User::builder()
                    .id("my-sub")
                    .username("firstname.lastname")
                    .enabled(true)
                    .with_role("main-entrance")
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .build();
                connector
                    .replace_api_mock(KentixApiMocker::default().can_get_all_levelprofiles().can_get_all_users())
                    .await;
                assert!(connector.all_users().await.unwrap().is_empty(), "Sanity: The mapping is empty beforehand.");

                // when
                let created_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                // then
                assert!(matches!(created_result, Err(kids_lib::error::KidsError::RequestFailed(_, _))));
            }

            #[rstest::rstest]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                    .id("my-sub")
                    .enabled(true)
                    .username("firstname.lastname")
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "this-is-not-a-hex-string")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .build())]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                    .id("my-sub")
                    .enabled(true)
                    .username("firstname.lastname")
                    // This is a valid hex string, but the RFID UID is a 128 bit integer while this one here requires 132 bits (33 hex chars).
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .build())]
            async fn create_user_fails_with_invalid_rfid_uid(
                mut connector: Connector,
                #[case] source_user: impl kids_lib::interface::source::User + Send + Sync + 'static,
            ) {
                // given
                connector
                    .replace_api_mock(KentixApiMocker::default().can_get_all_levelprofiles().can_get_all_users())
                    .await;
                assert!(connector.all_users().await.unwrap().is_empty(), "Sanity: The mapping is empty beforehand.");

                // when
                let created_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                // then
                assert!(matches!(created_result, Err(kids_lib::error::KidsError::RequestFailed(_, _))));
            }

            #[rstest::rstest]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                    .id("my-sub")
                    .enabled(true)
                    .username("firstname.lastname")
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "a7b")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .build())]
            #[tokio::test]
            #[case(kids_test_lib::User::builder()
                    .id("my-sub")
                    .enabled(true)
                    .username("firstname.lastname")
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "789")
                    .build())]
            async fn create_user_fails_with_multiple_rfid_uid_or_data(
                mut connector: Connector,
                #[case] source_user: impl kids_lib::interface::source::User + Send + Sync + 'static,
            ) {
                // given
                connector
                    .replace_api_mock(KentixApiMocker::default().can_get_all_levelprofiles().can_get_all_users())
                    .await;
                assert!(connector.all_users().await.unwrap().is_empty(), "Sanity: The mapping is empty beforehand.");

                // when
                let created_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                // then
                assert!(matches!(created_result, Err(kids_lib::error::KidsError::RequestFailed(_, _))));
            }

            #[rstest::rstest]
            #[tokio::test]
            async fn create_user_fails_without_username(mut connector: Connector) {
                // given
                let source_user = kids_test_lib::User::builder()
                    .id("my-sub")
                    .enabled(true)
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .build();
                connector
                    .replace_api_mock(KentixApiMocker::default().can_get_all_levelprofiles().can_get_all_users())
                    .await;
                assert!(connector.all_users().await.unwrap().is_empty(), "Sanity: The mapping is empty beforehand.");

                // when
                let created_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                // then
                assert!(matches!(created_result, Err(kids_lib::error::KidsError::RequestFailed(_, _))));
            }
        }

        mod update {
            use super::*;

            #[rstest::rstest]
            #[tokio::test]
            async fn updates_user(mut connector: Connector) {
                // given
                let available_levelprofiles = [
                    crate::target::dto::Levelprofile::builder().id(43).name("main-entrance").build(),
                    crate::target::dto::Levelprofile::builder().id(47).name("server-room").build(),
                ];
                let kentix_user_before = crate::target::dto::UserWithId::builder()
                    .id(53)
                    .user(
                        crate::target::dto::User::builder()
                            .username("my-sub")
                            .fullname("firstname.lastname")
                            .is_active(true)
                            .with_levelprofile(43)
                            .emergency_access(false)
                            .rfid_uid(123)
                            .rfid_data("456")
                            .build(),
                    )
                    .build();
                let source_user = kids_test_lib::User::builder()
                    .id("my-sub")
                    .username("firstname.lastname-newname")
                    .first_name("Firstname")
                    .last_name("Lastname")
                    .enabled(false)
                    .email("user@example.com")
                    .with_role("server-room")
                    .with_role(OFFLINE_ACCESS_ROLE)
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .build();
                let expected_kentix_user_after = crate::target::dto::UserWithId::builder()
                    .id(53)
                    .user(
                        crate::target::dto::User::builder()
                            .username("my-sub")
                            .fullname("firstname.lastname-newname")
                            .is_active(false)
                            .email("user@example.com")
                            .with_levelprofile(47)
                            .emergency_access(true)
                            .rfid_uid(123)
                            .rfid_data("456")
                            .build(),
                    )
                    .build();
                connector
                    .replace_api_mock(
                        KentixApiMocker::default()
                            .with_levelprofiles(available_levelprofiles)
                            .with_users([kentix_user_before.clone()])
                            .can_get_all_levelprofiles()
                            .can_get_all_users()
                            .require_update_user(expected_kentix_user_after.clone()),
                    )
                    .await;
                assert_eq!(
                    *connector.users.users().get(&kentix_user_before.user.username).unwrap(),
                    kentix_user_before,
                    "Sanity: The old user is in the mapping."
                );

                // when
                let update_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                // then
                update_result.expect("Error updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.len() == 1);
                let updated_user_id = all_users.iter().next().unwrap();
                assert_eq!(updated_user_id, &expected_kentix_user_after.user.username.0);
                let (updated_user_id, updated_user) = connector.users.users().iter().next().unwrap();
                assert_eq!(*updated_user_id, expected_kentix_user_after.user.username);
                assert_eq!(updated_user, &expected_kentix_user_after);
            }

            #[rstest::rstest]
            #[tokio::test]
            async fn no_call_to_kentix_when_up_to_date(mut connector: Connector) {
                // given
                let source_user = kids_test_lib::User::builder()
                    .id("my-sub")
                    .username("firstname.lastname-newname")
                    .first_name("Firstname")
                    .last_name("Lastname")
                    .enabled(false)
                    .email("user@example.com")
                    .with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b")
                    .with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456")
                    .build();
                let kentix_user = crate::target::dto::UserWithId::builder()
                    .id(59)
                    .user(
                        crate::target::dto::User::builder()
                            .username("my-sub")
                            .fullname("firstname.lastname-newname")
                            .is_active(false)
                            .email("user@example.com")
                            .emergency_access(false)
                            .rfid_uid(123)
                            .rfid_data("456")
                            .build(),
                    )
                    .build();
                connector
                    .replace_api_mock(
                        KentixApiMocker::default()
                            .with_users([kentix_user.clone()])
                            .can_get_all_levelprofiles()
                            .can_get_all_users()
                            .errors_update_user(kentix_user.clone()),
                    )
                    .await;
                assert_eq!(
                    *connector.users.users().get(&kentix_user.user.username).unwrap(),
                    kentix_user,
                    "Sanity: The old user is in the mapping."
                );

                // when
                let update_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                // then
                update_result.expect("Error updating user");
                let all_users = connector.all_users().await.unwrap();
                assert!(all_users.len() == 1);
                let updated_user_id = all_users.iter().next().unwrap();
                assert_eq!(updated_user_id, &kentix_user.user.username.0);
                let (updated_user_id, updated_user) = connector.users.users().iter().next().unwrap();
                assert_eq!(*updated_user_id, kentix_user.user.username);
                assert_eq!(updated_user, &kentix_user);
            }

            #[rstest::rstest]
            #[tokio::test]
            async fn deletes_user_when_rfid_uid_or_data_no_longer_present(mut connector: Connector) {
                // given
                let available_levelprofiles = [
                    crate::target::dto::Levelprofile::builder().id(61).name("main-entrance").build(),
                    crate::target::dto::Levelprofile::builder().id(67).name("server-room").build(),
                ];
                let kentix_user_before = crate::target::dto::UserWithId::builder()
                    .id(71)
                    .user(
                        crate::target::dto::User::builder()
                            .username("my-sub")
                            .fullname("firstname.lastname")
                            .is_active(true)
                            .with_levelprofile(61)
                            .emergency_access(false)
                            .rfid_uid(123)
                            .rfid_data("456")
                            .build(),
                    )
                    .build();
                let get_source_user_builder = || {
                    kids_test_lib::User::builder()
                        .id("my-sub")
                        .username("firstname.lastname-newname")
                        .enabled(true)
                        .with_role("main-entrance")
                };
                enum Set {
                    None,
                    Uid,
                    Data, /* setting both is handled above in the "normal" case */
                }
                for set in [Set::None, Set::Uid, Set::Data] {
                    let source_user = match set {
                        Set::None => get_source_user_builder().build(),
                        Set::Uid => get_source_user_builder().with_attribute(RFID_UID_ATTRIBUTE_NAME, "7b").build(),
                        Set::Data => get_source_user_builder().with_attribute(RFID_DATA_ATTRIBUTE_NAME, "456").build(),
                    };
                    connector
                        .replace_api_mock(
                            KentixApiMocker::default()
                                .with_levelprofiles(available_levelprofiles.clone())
                                .with_users([kentix_user_before.clone()])
                                .can_get_all_levelprofiles()
                                .can_get_all_users()
                                .require_delete_user(kentix_user_before.clone()),
                        )
                        .await;
                    assert_eq!(
                        *connector.users.users().get(&kentix_user_before.user.username).unwrap(),
                        kentix_user_before,
                        "Sanity: The old user is in the mapping."
                    );

                    // when
                    let update_result = connector.create_or_update_user(std::sync::Arc::new(source_user)).await;

                    // then
                    update_result.expect("The operation should be successful...");
                    assert!(connector.all_users().await.unwrap().is_empty(), "...but the mapping should now be empty.");
                }
            }
        }

        mod delete {
            use super::*;

            #[rstest::rstest]
            #[tokio::test]
            async fn deletes_existing_user(mut connector: Connector) {
                // given
                let source_user_id = "my-sub".to_owned();
                let kentix_user = crate::target::dto::UserWithId::builder()
                    .id(73)
                    .user(
                        crate::target::dto::User::builder()
                            .username(source_user_id.as_str())
                            .fullname("firstname.lastname")
                            .is_active(false)
                            .email("user@example.com")
                            .emergency_access(false)
                            .rfid_uid(123)
                            .rfid_data("456")
                            .build(),
                    )
                    .build();
                connector
                    .replace_api_mock(
                        KentixApiMocker::default()
                            .with_users([kentix_user.clone()])
                            .can_get_all_levelprofiles()
                            .can_get_all_users()
                            .require_delete_user(kentix_user.clone()),
                    )
                    .await;
                assert_eq!(
                    *connector.users.users().get(&kentix_user.user.username).unwrap(),
                    kentix_user,
                    "Sanity: The old user is in the mapping."
                );

                // when
                let delete_result = connector.delete_user(&source_user_id).await;

                // then
                delete_result.expect("Error deleting user");
                assert!(connector.all_users().await.unwrap().is_empty());
                assert!(connector.users.users().is_empty());
            }

            #[rstest::rstest]
            #[tokio::test]
            async fn delete_user_fails_nonexistent(mut connector: Connector) {
                // given
                let source_user_id = "my-sub".to_owned();
                connector
                    .replace_api_mock(KentixApiMocker::default().can_get_all_levelprofiles().can_get_all_users())
                    .await;
                assert!(connector.all_users().await.unwrap().is_empty(), "Sanity: The mapping is empty beforehand.");

                // when
                let delete_result = connector.delete_user(&source_user_id).await;

                // then
                assert!(matches!(delete_result, Err(kids_lib::error::KidsError::RequestFailed(_, _))));
            }
        }
    }
}
