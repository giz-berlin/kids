use crate::source::interface;
use crate::source::keycloak::{external, group};
use crate::{error, types};
use std::collections;

pub struct KeycloakUser {
    pub keycloak_api: std::sync::Arc<dyn external::KeycloakApi + Send + Sync>,
    pub user_representation: keycloak::types::UserRepresentation,
}

impl KeycloakUser {
    pub fn new(keycloak_api: std::sync::Arc<dyn external::KeycloakApi + Send + Sync>, user_representation: keycloak::types::UserRepresentation) -> Self {
        KeycloakUser {
            keycloak_api,
            user_representation,
        }
    }

    pub fn from_webhook_user(keycloak_api: std::sync::Arc<dyn external::KeycloakApi + Send + Sync>, webhook_user: KeycloakWebhookUser) -> Self {
        KeycloakUser {
            keycloak_api: keycloak_api,
            user_representation: keycloak::types::UserRepresentation {
                access: None,
                application_roles: None,
                attributes: Some(webhook_user.attributes),
                client_consents: None,
                client_roles: None,
                created_timestamp: None,
                credentials: None,
                disableable_credential_types: None,
                email: Some(webhook_user.email),
                email_verified: None,
                enabled: Some(webhook_user.enabled),
                federated_identities: None,
                federation_link: None,
                first_name: None,
                groups: None,
                id: Some(webhook_user.id),
                last_name: None,
                not_before: None,
                origin: None,
                realm_roles: Some(webhook_user.realm_roles),
                required_actions: None,
                self_: None,
                service_account_client_id: None,
                social_links: None,
                totp: None,
                user_profile_metadata: None,
                username: Some(webhook_user.username),
            },
        }
    }
}

#[async_trait::async_trait]
impl interface::User for KeycloakUser {
    fn id(&self) -> &types::SharedResourceIdentifier {
        // We can unwrap here because every Keycloak user has got an ID.
        self.user_representation.id.as_ref().unwrap()
    }

    fn enabled(&self) -> bool {
        // We can unwrap here because every Keycloak will always tell us whether users are enabled.
        self.user_representation.enabled.unwrap()
    }

    fn username(&self) -> Option<&str> {
        self.user_representation.username.as_deref()
    }

    fn email(&self) -> Option<&str> {
        self.user_representation.email.as_deref()
    }

    fn attributes(&self) -> &collections::HashMap<String, Vec<String>> {
        self.user_representation.attributes.as_ref().unwrap()
    }

    fn roles(&self) -> &Vec<String> {
        self.user_representation.realm_roles.as_ref().unwrap()
    }

    async fn groups(&self) -> Result<Vec<std::sync::Arc<dyn interface::Group + Send + Sync>>, error::KidsError> {
        let users = self.keycloak_api.get_groups_of_user(self.id()).await?;
        Ok(users
            .into_iter()
            .map(|g| std::sync::Arc::new(group::KeycloakGroup::new(self.keycloak_api.clone(), g)) as std::sync::Arc<dyn interface::Group + Send + Sync>)
            .collect())
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct KeycloakWebhookUser {
    pub id: String,
    pub enabled: bool,
    pub username: String,
    pub email: String,
    pub attributes: std::collections::HashMap<String, Vec<String>>,
    pub realm_roles: Vec<String>,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::source::interface::User;
    use crate::test_util::constants;
    use mockall::predicate;

    #[tokio::test]
    async fn test_user_groups() {
        // given
        let mut mock = external::MockKeycloakApi::new();
        mock.expect_get_groups_of_user().with(predicate::eq(constants::DEFAULT_USER_ID)).returning(|_| {
            Ok(vec![
                external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::DEFAULT_GROUP_ID)
                    .build_into(),
                external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::ANOTHER_GROUP_ID)
                    .build_into(),
            ])
        });

        let user = KeycloakUser::new(
            std::sync::Arc::new(mock),
            external::test::KeycloakUserRepresentationBuilder::default()
                .id(constants::DEFAULT_USER_ID)
                .build_into(),
        );

        // when
        let user_groups = user.groups().await.unwrap();

        // then
        assert_eq!(user_groups.len(), 2);
        assert_eq!(user_groups[0].id(), constants::DEFAULT_GROUP_ID);
        assert_eq!(user_groups[1].id(), constants::ANOTHER_GROUP_ID);
    }
}
