use crate::source::interface;
use crate::source::keycloak::{external, group};
use crate::{error, types};
use std::{collections, rc};

pub struct KeycloakUser {
    pub keycloak_api: rc::Rc<dyn external::KeycloakApi>,
    pub user_representation: keycloak::types::UserRepresentation,
}

impl KeycloakUser {
    pub fn new(keycloak_api: rc::Rc<dyn external::KeycloakApi>, user_representation: keycloak::types::UserRepresentation) -> Self {
        KeycloakUser {
            keycloak_api,
            user_representation,
        }
    }
}

#[async_trait::async_trait(?Send)]
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

    async fn groups(&self) -> Result<Vec<rc::Rc<dyn interface::Group>>, error::KidsError> {
        let users = self.keycloak_api.get_groups_of_user(self.id()).await?;
        Ok(users
            .into_iter()
            .map(|g| rc::Rc::new(group::KeycloakGroup::new(self.keycloak_api.clone(), g)) as rc::Rc<dyn interface::Group>)
            .collect())
    }
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
            rc::Rc::new(mock),
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
