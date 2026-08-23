use kids_lib::error;

use kids_lib::interface::source::Group;

/// A connector to Keycloak providing the [Source](interface::Source) interface.
pub struct Connector {
    pub keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi + Send + Sync>,
}

#[derive(serde::Deserialize)]
pub struct KeycloakConfig {
    pub keycloak_api: crate::external::KeycloakApiConfig,
}

#[async_trait::async_trait]
impl kids_lib::interface::source::Source for Connector {
    type Config = KeycloakConfig;
    type UserWebhookPayload = crate::user::KeycloakWebhookUser;
    type GroupWebhookPayload = crate::group::KeycloakWebhookGroup;

    fn info(&self) -> String {
        "Keycloak Connector!".to_string()
    }

    fn new(config: Self::Config) -> Self {
        Connector {
            keycloak_api: std::sync::Arc::new(crate::external::KeycloakServiceAccountClient::new(config.keycloak_api)),
        }
    }

    async fn all_groups(&self) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>>, error::KidsError> {
        let root_groups = self.keycloak_api.get_groups().await?;

        let mut result: Vec<std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>> = Vec::new();
        let mut queue: std::collections::VecDeque<std::sync::Arc<crate::group::KeycloakGroup>> = std::collections::VecDeque::new();

        for root_group in root_groups {
            let group_instance = std::sync::Arc::new(crate::group::KeycloakGroup::new_from_group_representation(
                self.keycloak_api.clone(),
                root_group,
            ));
            queue.push_back(group_instance.clone());
            result.push(group_instance);
        }

        while let Some(parent) = queue.pop_front() {
            for subgroup in self.keycloak_api.get_subgroups(parent.id()).await? {
                let group_instance = std::sync::Arc::new(crate::group::KeycloakGroup::new_with_parent(
                    self.keycloak_api.clone(),
                    subgroup,
                    parent.clone(),
                ));
                queue.push_back(group_instance.clone());
                result.push(group_instance);
            }
        }

        Ok(result)
    }

    async fn all_users(&self) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>>, error::KidsError> {
        let users = self.keycloak_api.get_users().await?;
        Ok(users
            .into_iter()
            .map(|u| {
                std::sync::Arc::new(crate::user::KeycloakUser::from_user_representation(self.keycloak_api.clone(), u))
                    as std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>
            })
            .collect())
    }

    async fn user_from_webhook(
        &self,
        webhook_user: Self::UserWebhookPayload,
    ) -> Result<Box<dyn kids_lib::interface::source::User + Send + Sync>, error::KidsError> {
        Ok(Box::new(
            crate::user::KeycloakUser::from_webhook_user(self.keycloak_api.clone(), webhook_user).await?,
        ))
    }

    fn group_from_webhook(&self, webhook_group: Self::GroupWebhookPayload) -> Box<dyn kids_lib::interface::source::Group + Send + Sync> {
        Box::new(crate::group::KeycloakGroup::from_webhook_group(self.keycloak_api.clone(), webhook_group))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use kids_lib::interface::source::Source;
    use kids_test_lib::util::constants;
    use mockall::predicate;

    #[tokio::test]
    async fn test_all_users() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        mock.expect_get_users().returning(|| {
            Ok(vec![
                crate::external::test::KeycloakUserRepresentationBuilder::default()
                    .id(constants::DEFAULT_SOURCE_USER_ID)
                    .build_into(),
                crate::external::test::KeycloakUserRepresentationBuilder::default()
                    .id(constants::ANOTHER_SOURCE_USER_ID)
                    .build_into(),
            ])
        });

        let source = Connector {
            keycloak_api: std::sync::Arc::new(mock),
        };

        // when
        let users = source.all_users().await.unwrap();

        // then
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].id(), constants::DEFAULT_SOURCE_USER_ID);
        assert_eq!(users[1].id(), constants::ANOTHER_SOURCE_USER_ID);
    }

    #[tokio::test]
    async fn test_all_groups() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        mock.expect_get_groups().returning(|| {
            Ok(vec![
                crate::external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::DEFAULT_SOURCE_GROUP_ID)
                    .build_into(),
                crate::external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::ANOTHER_SOURCE_GROUP_ID)
                    .build_into(),
            ])
        });
        mock.expect_get_subgroups().returning(|_| Ok(vec![]));

        let source = Connector {
            keycloak_api: std::sync::Arc::new(mock),
        };

        // when
        let groups = source.all_groups().await.unwrap();

        // then
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id(), constants::DEFAULT_SOURCE_GROUP_ID);
        assert_eq!(groups[1].id(), constants::ANOTHER_SOURCE_GROUP_ID);
    }

    #[tokio::test]
    async fn test_all_groups_recursive() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        mock.expect_get_groups().returning(|| {
            Ok(vec![
                crate::external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::DEFAULT_SOURCE_GROUP_ID)
                    .build_into(),
            ])
        });
        mock.expect_get_subgroups()
            .with(predicate::eq(constants::DEFAULT_SOURCE_GROUP_ID))
            .returning(|_| {
                Ok(vec![
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::ANOTHER_SOURCE_GROUP_ID)
                        .build_into(),
                ])
            });
        mock.expect_get_subgroups()
            .with(predicate::eq(constants::ANOTHER_SOURCE_GROUP_ID))
            .returning(|_| Ok(vec![]));

        let source = Connector {
            keycloak_api: std::sync::Arc::new(mock),
        };

        // when
        let groups = source.all_groups().await.unwrap();

        // then
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id(), constants::DEFAULT_SOURCE_GROUP_ID);
        assert_eq!(groups[1].id(), constants::ANOTHER_SOURCE_GROUP_ID);
    }
}
