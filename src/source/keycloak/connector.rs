use crate::error;
use crate::source::interface;
use crate::source::keycloak::group::KeycloakGroup;
use crate::source::keycloak::user::KeycloakUser;
use crate::source::keycloak::{external, group, user};
use std::sync;

/// A connector to Keycloak providing the [Source](interface::Source) interface.
pub struct Connector {
    pub keycloak_api: sync::Arc<dyn external::KeycloakApi + Send + Sync>,
}

#[derive(serde::Deserialize)]
pub struct KeycloakConfig {
    pub keycloak_api: external::KeycloakApiConfig,
}

#[async_trait::async_trait]
impl interface::Source for Connector {
    type Config = KeycloakConfig;
    type UserWebhookPayload = user::KeycloakWebhookUser;
    type GroupWebhookPayload = group::KeycloakWebhookGroup;

    fn info(&self) -> String {
        "Keycloak Connector!".to_string()
    }

    fn new(config: Self::Config) -> Self {
        Connector {
            keycloak_api: sync::Arc::new(external::KeycloakServiceAccountClient::new(config.keycloak_api)),
        }
    }

    async fn all_groups(&self) -> Result<Vec<sync::Arc<dyn interface::Group>>, error::KidsError> {
        let groups = self.keycloak_api.get_groups().await?;
        Ok(groups
            .into_iter()
            .map(|group| sync::Arc::new(group::KeycloakGroup::new(self.keycloak_api.clone(), group)) as sync::Arc<dyn interface::Group>)
            .collect())
    }

    async fn all_users(&self) -> Result<Vec<Box<dyn interface::User>>, error::KidsError> {
        let users = self.keycloak_api.get_users().await?;
        Ok(users
            .into_iter()
            .map(|u| Box::new(user::KeycloakUser::new(self.keycloak_api.clone(), u)) as Box<dyn interface::User>)
            .collect())
    }

    fn user_from_webhook(&self, webhook_user: Self::UserWebhookPayload) -> Box<dyn interface::User + Send + Sync> {
        Box::new(KeycloakUser::from_webhook_user(self.keycloak_api.clone(), webhook_user))
    }

    fn group_from_webhook(&self, webhook_group: Self::GroupWebhookPayload) -> Box<dyn interface::Group + Send + Sync> {
        Box::new(KeycloakGroup::from_webhook_group(self.keycloak_api.clone(), webhook_group))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::source::interface::Source;
    use crate::test_util::constants;

    #[tokio::test]
    async fn test_all_users() {
        // given
        let mut mock = external::MockKeycloakApi::new();
        mock.expect_get_users().returning(|| {
            Ok(vec![
                external::test::KeycloakUserRepresentationBuilder::default()
                    .id(constants::DEFAULT_USER_ID)
                    .build_into(),
                external::test::KeycloakUserRepresentationBuilder::default()
                    .id(constants::ANOTHER_USER_ID)
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
        assert_eq!(users[0].id(), constants::DEFAULT_USER_ID);
        assert_eq!(users[1].id(), constants::ANOTHER_USER_ID);
    }

    #[tokio::test]
    async fn test_all_groups() {
        // given
        let mut mock = external::MockKeycloakApi::new();
        mock.expect_get_groups().returning(|| {
            Ok(vec![
                external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::DEFAULT_GROUP_ID)
                    .build_into(),
                external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::ANOTHER_GROUP_ID)
                    .build_into(),
            ])
        });

        let source = Connector {
            keycloak_api: std::sync::Arc::new(mock),
        };

        // when
        let groups = source.all_groups().await.unwrap();

        // then
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id(), constants::DEFAULT_GROUP_ID);
        assert_eq!(groups[1].id(), constants::ANOTHER_GROUP_ID);
    }
}
