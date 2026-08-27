use std::collections;

use kids_lib::error::KidsError;

pub struct KeycloakUser {
    pub keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi + Send + Sync>,

    id: String,
    enabled: bool,
    username: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    attributes: std::collections::HashMap<String, Vec<String>>,
}

impl KeycloakUser {
    pub fn from_user_representation(
        keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi + Send + Sync>,
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
            first_name: user_representation.first_name,
            last_name: user_representation.last_name,
            email: user_representation.email,
            // Users may or may not have attributes so use the default value (an empty map) as the fallback.
            // Whether the attributes are actually required depends on the target (e.g. if they store additional metadata about
            // the user mapping in the user's attributes).
            attributes: user_representation.attributes.unwrap_or_default(),
        }
    }

    pub async fn from_webhook_user(
        keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi + Send + Sync>,
        webhook_user: KeycloakWebhookUser,
    ) -> Result<Self, KidsError> {
        let user = keycloak_api.get_user(&webhook_user.id).await?;
        Ok(KeycloakUser {
            keycloak_api,
            id: webhook_user.id,
            enabled: webhook_user.enabled,
            username: webhook_user.username,
            first_name: user.first_name,
            last_name: user.last_name,
            email: webhook_user.email,
            attributes: webhook_user.attributes,
        })
    }
}

#[async_trait::async_trait]
impl kids_lib::interface::source::User for KeycloakUser {
    fn id(&self) -> &kids_lib::types::SharedResourceIdentifier {
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

    fn first_name(&self) -> Option<&str> {
        self.first_name.as_deref()
    }

    fn last_name(&self) -> Option<&str> {
        self.last_name.as_deref()
    }

    fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }

    fn attributes(&self) -> &collections::HashMap<String, Vec<String>> {
        &self.attributes
    }

    async fn groups(&self, include_transitive_groups: bool) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>>, KidsError> {
        let direct_groups = self.keycloak_api.get_groups_of_user(self.id()).await?;

        if !include_transitive_groups {
            return Ok(direct_groups
                .into_iter()
                .map(|g| {
                    std::sync::Arc::new(crate::group::KeycloakGroup::new_from_group_representation(self.keycloak_api.clone(), g))
                        as std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>
                })
                .collect());
        }

        let mut cache = collections::HashMap::new();
        for direct_group in direct_groups {
            crate::group::resolve_group_with_ancestors(self.keycloak_api.clone(), direct_group, &mut cache).await?;
        }

        // Since `cache` ends up holding every direct group and all of their ancestors it already is the (deduplicated) result we want.
        Ok(cache
            .into_values()
            .map(|g| g as std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>)
            .collect())
    }

    async fn roles(&self) -> Result<Vec<String>, KidsError> {
        let client_roles = self.keycloak_api.get_user_client_roles(self.id()).await?;
        let roles = client_roles.into_iter().filter_map(|role| role.name).collect();
        Ok(roles)
    }
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct KeycloakWebhookUser {
    pub id: String,
    pub enabled: bool,
    pub username: Option<String>,
    pub email: Option<String>,
    pub attributes: std::collections::HashMap<String, Vec<String>>,
}

#[cfg(test)]
mod test {
    use super::*;
    use kids_lib::interface::source::User;
    use kids_test_lib::util::constants;
    use mockall::predicate;

    #[tokio::test]
    async fn test_user_groups() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        mock.expect_get_groups_of_user()
            .with(predicate::eq(constants::DEFAULT_SOURCE_USER_ID))
            .returning(|_| {
                Ok(vec![
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::DEFAULT_SOURCE_GROUP_ID)
                        .build_into(),
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::ANOTHER_SOURCE_GROUP_ID)
                        .build_into(),
                ])
            });

        let user = KeycloakUser::from_user_representation(
            std::sync::Arc::new(mock),
            crate::external::test::KeycloakUserRepresentationBuilder::default()
                .id(constants::DEFAULT_SOURCE_USER_ID)
                .build_into(),
        );

        // when
        let user_groups = user.groups(false).await.unwrap();

        // then
        assert_eq!(user_groups.len(), 2);
        assert_eq!(user_groups[0].id(), constants::DEFAULT_SOURCE_GROUP_ID);
        assert_eq!(user_groups[1].id(), constants::ANOTHER_SOURCE_GROUP_ID);
    }

    /// Asserts that `user_groups` contains exactly the given IDs, once each (order does not matter, but duplicates or
    /// missing entries do), by comparing sorted ID lists.
    fn assert_group_ids(user_groups: &[std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>], expected_ids: &[&str]) {
        let mut actual_ids: Vec<&str> = user_groups.iter().map(|g| g.id().as_str()).collect();
        actual_ids.sort_unstable();
        let mut expected_ids = expected_ids.to_vec();
        expected_ids.sort_unstable();
        assert_eq!(actual_ids, expected_ids);
    }

    #[tokio::test]
    async fn test_user_groups_transitive() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        // the direct group already carries its `parent_id`, so resolving it must not fetch it again via `get_group`
        mock.expect_get_groups_of_user()
            .with(predicate::eq(constants::DEFAULT_SOURCE_USER_ID))
            .returning(|_| {
                Ok(vec![
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::DEFAULT_SOURCE_GROUP_ID)
                        .parent_id(constants::ANOTHER_SOURCE_GROUP_ID)
                        .build_into(),
                ])
            });
        mock.expect_get_group()
            .with(predicate::eq(constants::ANOTHER_SOURCE_GROUP_ID))
            .times(1)
            .returning(|_| {
                Ok(crate::external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::ANOTHER_SOURCE_GROUP_ID)
                    .parent_id(constants::THIRD_SOURCE_GROUP_ID)
                    .build_into())
            });
        mock.expect_get_group()
            .with(predicate::eq(constants::THIRD_SOURCE_GROUP_ID))
            .times(1)
            .returning(|_| {
                Ok(crate::external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::THIRD_SOURCE_GROUP_ID)
                    .build_into())
            });

        let user = KeycloakUser::from_user_representation(
            std::sync::Arc::new(mock),
            crate::external::test::KeycloakUserRepresentationBuilder::default()
                .id(constants::DEFAULT_SOURCE_USER_ID)
                .build_into(),
        );

        // when
        let user_groups = user.groups(true).await.unwrap();

        // then
        // the direct group's parents are included, order is not guaranteed
        assert_group_ids(
            &user_groups,
            &[
                constants::THIRD_SOURCE_GROUP_ID,
                constants::ANOTHER_SOURCE_GROUP_ID,
                constants::DEFAULT_SOURCE_GROUP_ID,
            ],
        );
    }

    #[tokio::test]
    async fn test_user_groups_transitive_deduplicates_shared_ancestors() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        // the user is directly in both a group and one of its ancestors
        mock.expect_get_groups_of_user()
            .with(predicate::eq(constants::DEFAULT_SOURCE_USER_ID))
            .returning(|_| {
                Ok(vec![
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::ANOTHER_SOURCE_GROUP_ID)
                        .parent_id(constants::THIRD_SOURCE_GROUP_ID)
                        .build_into(),
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::DEFAULT_SOURCE_GROUP_ID)
                        .parent_id(constants::ANOTHER_SOURCE_GROUP_ID)
                        .build_into(),
                ])
            });
        // THIRD_GROUP_ID is the shared ancestor of both direct groups
        mock.expect_get_group()
            .with(predicate::eq(constants::THIRD_SOURCE_GROUP_ID))
            .times(1)
            .returning(|_| {
                Ok(crate::external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::THIRD_SOURCE_GROUP_ID)
                    .build_into())
            });

        let user = KeycloakUser::from_user_representation(
            std::sync::Arc::new(mock),
            crate::external::test::KeycloakUserRepresentationBuilder::default()
                .id(constants::DEFAULT_SOURCE_USER_ID)
                .build_into(),
        );

        // when
        let user_groups = user.groups(true).await.unwrap();

        // then
        assert_group_ids(
            &user_groups,
            &[
                constants::THIRD_SOURCE_GROUP_ID,
                constants::ANOTHER_SOURCE_GROUP_ID,
                constants::DEFAULT_SOURCE_GROUP_ID,
            ],
        );
    }

    #[tokio::test]
    async fn test_user_groups_transitive_deduplicates_shared_parent_of_siblings() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        // the user is directly in two sibling groups (THIRD_GROUP_ID -> ANOTHER_GROUP_ID and THIRD_GROUP_ID -> DEFAULT_GROUP_ID)
        mock.expect_get_groups_of_user()
            .with(predicate::eq(constants::DEFAULT_SOURCE_USER_ID))
            .returning(|_| {
                Ok(vec![
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::ANOTHER_SOURCE_GROUP_ID)
                        .parent_id(constants::THIRD_SOURCE_GROUP_ID)
                        .build_into(),
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::DEFAULT_SOURCE_GROUP_ID)
                        .parent_id(constants::THIRD_SOURCE_GROUP_ID)
                        .build_into(),
                ])
            });
        // THIRD_GROUP_ID is the shared parent of both direct groups
        mock.expect_get_group()
            .with(predicate::eq(constants::THIRD_SOURCE_GROUP_ID))
            .times(1)
            .returning(|_| {
                Ok(crate::external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(constants::THIRD_SOURCE_GROUP_ID)
                    .build_into())
            });

        let user = KeycloakUser::from_user_representation(
            std::sync::Arc::new(mock),
            crate::external::test::KeycloakUserRepresentationBuilder::default()
                .id(constants::DEFAULT_SOURCE_USER_ID)
                .build_into(),
        );

        // when
        let user_groups = user.groups(true).await.unwrap();

        // then
        assert_group_ids(
            &user_groups,
            &[
                constants::THIRD_SOURCE_GROUP_ID,
                constants::ANOTHER_SOURCE_GROUP_ID,
                constants::DEFAULT_SOURCE_GROUP_ID,
            ],
        );
    }
}
