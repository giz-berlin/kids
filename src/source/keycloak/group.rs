use crate::source::interface;
use crate::source::keycloak::external;
use crate::{error, types};
use std::rc;

pub struct KeycloakGroup {
    pub keycloak_api: rc::Rc<dyn external::KeycloakApi>,
    pub group_representation: keycloak::types::GroupRepresentation,
    pub parent_group: Option<rc::Rc<KeycloakGroup>>,
    pub root_group: Option<rc::Rc<KeycloakGroup>>,
}

impl KeycloakGroup {
    pub fn new(keycloak_api: rc::Rc<dyn external::KeycloakApi>, group_representation: keycloak::types::GroupRepresentation) -> Self {
        KeycloakGroup {
            keycloak_api,
            group_representation,
            parent_group: None,
            root_group: None,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl interface::Group for KeycloakGroup {
    fn id(&self) -> &types::SharedResourceIdentifier {
        // We can unwrap here because every Keycloak group has got an ID.
        self.group_representation.id.as_ref().unwrap()
    }

    fn name(&self) -> &str {
        // We can unwrap here because every Keycloak group has got a name.
        self.group_representation.name.as_ref().unwrap()
    }

    fn path(&self) -> &str {
        // We can unwrap here because every Keycloak group has got a path.
        self.group_representation.path.as_ref().unwrap()
    }

    fn root_group(self: rc::Rc<Self>) -> rc::Rc<dyn interface::Group> {
        match &self.root_group {
            Some(root_group) => root_group.clone(),
            None => self,
        }
    }

    fn parent_group(&self) -> Option<rc::Rc<dyn interface::Group>> {
        match &self.parent_group {
            Some(parent_group) => Some(parent_group.clone()),
            None => None,
        }
    }

    async fn sub_groups(self: rc::Rc<Self>) -> Result<Vec<rc::Rc<dyn interface::Group>>, error::KidsError> {
        let sub_groups = self.keycloak_api.get_subgroups(self.group_representation.id.as_ref().unwrap()).await?;
        Ok(sub_groups
            .into_iter()
            .map(|g| {
                let mut group = KeycloakGroup::new(self.keycloak_api.clone(), g);
                group.parent_group = Some(self.clone());
                group.root_group = match &self.root_group {
                    Some(root) => Some(root.clone()),
                    None => Some(self.clone()),
                };
                rc::Rc::new(group) as rc::Rc<dyn interface::Group>
            })
            .collect())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::util::test_constants;
    use mockall::predicate;
    #[tokio::test]
    async fn test_sub_groups() {
        // given
        let mut mock = external::MockKeycloakApi::new();
        mock.expect_get_subgroups()
            .with(predicate::eq(test_constants::DEFAULT_GROUP_ID))
            .returning(|_| {
                Ok(vec![external::test::KeycloakGroupRepresentationBuilder::default()
                    .id(test_constants::ANOTHER_GROUP_ID)
                    .build_into()])
            });

        let group = rc::Rc::new(KeycloakGroup::new(
            rc::Rc::new(mock),
            external::test::KeycloakGroupRepresentationBuilder::default()
                .id(test_constants::DEFAULT_GROUP_ID)
                .build_into(),
        )) as rc::Rc<dyn interface::Group>;

        // when
        let sub_groups = group.clone().sub_groups().await.unwrap();

        // then
        assert_eq!(sub_groups.len(), 1);
        assert_eq!(sub_groups[0].id(), test_constants::ANOTHER_GROUP_ID);
        // also assigns parent relationships
        assert!(rc::Rc::ptr_eq(&sub_groups[0].parent_group().unwrap(), &group));
        assert!(rc::Rc::ptr_eq(&sub_groups[0].clone().root_group(), &group));
    }
}
