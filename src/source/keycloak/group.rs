use crate::source::interface;
use crate::source::keycloak::external;
use crate::{error, types};
use std::collections::HashMap;

pub struct KeycloakGroup {
    pub keycloak_api: std::sync::Arc<dyn external::KeycloakApi>,
    pub group_representation: keycloak::types::GroupRepresentation,
    pub parent_group: Option<std::sync::Arc<KeycloakGroup>>,
    pub root_group: Option<std::sync::Arc<KeycloakGroup>>,
}

impl KeycloakGroup {
    pub fn new(keycloak_api: std::sync::Arc<dyn external::KeycloakApi>, group_representation: keycloak::types::GroupRepresentation) -> Self {
        KeycloakGroup {
            keycloak_api,
            group_representation,
            parent_group: None,
            root_group: None,
        }
    }

    pub fn new_with_parent(
        keycloak_api: std::sync::Arc<dyn external::KeycloakApi>,
        group_representation: keycloak::types::GroupRepresentation,
        parent: std::sync::Arc<KeycloakGroup>,
    ) -> KeycloakGroup {
        let mut group = Self::new(keycloak_api, group_representation);
        group.root_group = match &parent.root_group {
            Some(root) => Some(root.clone()),
            None => Some(parent.clone()),
        };
        group.parent_group = Some(parent);
        group
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

    fn attributes(&self) -> &HashMap<String, Vec<String>> {
        self.group_representation.attributes.as_ref().unwrap()
    }

    fn root_group(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn interface::Group> {
        match &self.root_group {
            Some(root_group) => root_group.clone(),
            None => self,
        }
    }

    fn parent_group(&self) -> Option<std::sync::Arc<dyn interface::Group>> {
        match &self.parent_group {
            Some(parent_group) => Some(parent_group.clone()),
            None => None,
        }
    }

    async fn sub_groups(self: std::sync::Arc<Self>) -> Result<Vec<std::sync::Arc<dyn interface::Group>>, error::KidsError> {
        let sub_groups = self.keycloak_api.get_subgroups(self.id()).await?;
        Ok(sub_groups
            .into_iter()
            .map(|g| std::sync::Arc::new(KeycloakGroup::new_with_parent(self.keycloak_api.clone(), g, self.clone())) as std::sync::Arc<dyn interface::Group>)
            .collect())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_util::constants;
    use mockall::predicate;
    #[tokio::test]
    async fn test_sub_groups() {
        // given
        let mut mock = external::MockKeycloakApi::new();
        mock.expect_get_subgroups().with(predicate::eq(constants::DEFAULT_GROUP_ID)).returning(|_| {
            Ok(vec![external::test::KeycloakGroupRepresentationBuilder::default()
                .id(constants::ANOTHER_GROUP_ID)
                .build_into()])
        });

        let group = std::sync::Arc::new(KeycloakGroup::new(
            std::sync::Arc::new(mock),
            external::test::KeycloakGroupRepresentationBuilder::default()
                .id(constants::DEFAULT_GROUP_ID)
                .build_into(),
        )) as std::sync::Arc<dyn interface::Group>;

        // when
        let sub_groups = group.clone().sub_groups().await.unwrap();

        // then
        assert_eq!(sub_groups.len(), 1);
        assert_eq!(sub_groups[0].id(), constants::ANOTHER_GROUP_ID);
        // also assigns parent relationships
        assert!(std::sync::Arc::ptr_eq(&sub_groups[0].parent_group().unwrap(), &group));
        assert!(std::sync::Arc::ptr_eq(&sub_groups[0].clone().root_group(), &group));
    }
}
