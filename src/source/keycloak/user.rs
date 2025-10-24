use crate::source::interface;
use crate::source::keycloak::{external, group};
use crate::{error, types};
use std::collections;

pub struct KeycloakUser {
    pub keycloak_api: std::sync::Arc<dyn external::KeycloakApi + Send + Sync>,

    id: String,
    enabled: bool,
    username: Option<String>,
    email: Option<String>,
    attributes: std::collections::HashMap<String, Vec<String>>,
    realm_roles: Vec<String>,
}

impl KeycloakUser {
    pub fn from_user_representation(
        keycloak_api: std::sync::Arc<dyn external::KeycloakApi + Send + Sync>,
        user_representation: keycloak::types::UserRepresentation,
    ) -> Self {
        KeycloakUser {
            keycloak_api,
            // The Keycloak API library defines all attributes as optional, which in reality they shouldn't be.
            // Each Keycloak user must have an ID and so we expect the ID to be always set.
            id: user_representation.id.expect("Keycloak user is expected to have an ID"),
            // We expect the enabled flag for a user to be present as well.
            // While we could use `unwrap_or_default` here this would silently disable users since the default value of booleans is false.
            enabled: user_representation.enabled.expect("Keycloak user is expected to have an enabled attribute"),
            username: user_representation.username,
            email: user_representation.email,
            // Users may or may not have attributes so use the default value (an empty map) as the fallback.
            // Whether the attributes are actually required depends on the target (e.g. if they store additional metadata about
            // the user mapping in the user's attributes).
            attributes: user_representation.attributes.unwrap_or_default(),
            realm_roles: user_representation.realm_roles.unwrap_or_default(),
        }
    }

    pub fn from_webhook_user(keycloak_api: std::sync::Arc<dyn external::KeycloakApi + Send + Sync>, webhook_user: KeycloakWebhookUser) -> Self {
        KeycloakUser {
            keycloak_api,
            id: webhook_user.id,
            enabled: webhook_user.enabled,
            username: webhook_user.username,
            email: webhook_user.email,
            attributes: webhook_user.attributes,
            realm_roles: webhook_user.realm_roles,
        }
    }
}

#[async_trait::async_trait]
impl interface::User for KeycloakUser {
    fn id(&self) -> &types::SharedResourceIdentifier {
        // We can unwrap here because every Keycloak user has got an ID.
        &self.id
    }

    fn enabled(&self) -> bool {
        // We can unwrap here because every Keycloak will always tell us whether users are enabled.
        self.enabled
    }

    fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    fn attributes(&self) -> &collections::HashMap<String, Vec<String>> {
        &self.attributes
    }

    fn roles(&self) -> &Vec<String> {
        &self.realm_roles
    }

    async fn groups(&self) -> Result<Vec<std::sync::Arc<dyn interface::Group + Send + Sync>>, error::KidsError> {
        let users = self.keycloak_api.get_groups_of_user(self.id()).await?;
        Ok(users
            .into_iter()
            .map(|g| {
                std::sync::Arc::new(group::KeycloakGroup::new_from_group_representation(self.keycloak_api.clone(), g))
                    as std::sync::Arc<dyn interface::Group + Send + Sync>
            })
            .collect())
    }
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct KeycloakWebhookUser {
    pub id: String,
    pub enabled: bool,
    pub username: Option<String>,
    pub email: Option<String>,
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

        let user = KeycloakUser::from_user_representation(
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
