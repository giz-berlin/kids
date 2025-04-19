use crate::error;
use crate::source::interface;
use crate::source::keycloak::{external, group, user};
use std::rc;

/// A connector to Keycloak providing the [Source](interface::Source) interface.
pub struct Connector {
    pub keycloak_api: rc::Rc<dyn external::KeycloakApi>,
}

#[async_trait::async_trait(?Send)]
impl interface::Source for Connector {
    type Config = external::KeycloakConfig;

    fn info(&self) -> String {
        "Keycloak Connector!".to_string()
    }

    fn new(config: Self::Config) -> Self {
        Connector {
            keycloak_api: rc::Rc::new(external::KeycloakServiceAccountClient::new(config)),
        }
    }

    async fn all_groups(&self) -> Result<Vec<rc::Rc<dyn interface::Group>>, error::KidsError> {
        let groups = self.keycloak_api.get_groups().await?;
        Ok(groups
            .into_iter()
            .map(|group| rc::Rc::new(group::KeycloakGroup::new(self.keycloak_api.clone(), group)) as rc::Rc<dyn interface::Group>)
            .collect())
    }

    async fn all_users(&self) -> Result<Vec<Box<dyn interface::User>>, error::KidsError> {
        let users = self.keycloak_api.get_users().await?;
        Ok(users
            .into_iter()
            .map(|u| Box::new(user::KeycloakUser::new(self.keycloak_api.clone(), u)) as Box<dyn interface::User>)
            .collect())
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
            keycloak_api: rc::Rc::new(mock),
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
            keycloak_api: rc::Rc::new(mock),
        };

        // when
        let groups = source.all_groups().await.unwrap();

        // then
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id(), constants::DEFAULT_GROUP_ID);
        assert_eq!(groups[1].id(), constants::ANOTHER_GROUP_ID);
    }
}
