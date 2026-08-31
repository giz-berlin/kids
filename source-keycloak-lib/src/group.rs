use std::collections::HashMap;

use kids_lib::error::KidsError;

pub struct KeycloakGroup {
    pub keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi>,
    pub parent_group: Option<std::sync::Arc<KeycloakGroup>>,
    pub root_group: Option<std::sync::Arc<KeycloakGroup>>,

    id: String,
    name: String,
    path: String,
    attributes: std::collections::HashMap<String, Vec<String>>,
}

impl KeycloakGroup {
    pub fn new_from_group_representation(
        keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi>,
        group_representation: keycloak::types::GroupRepresentation,
    ) -> Self {
        Self {
            keycloak_api,
            parent_group: None,
            root_group: None,

            // We can unwrap here because every Keycloak group has got an ID.
            id: group_representation.id.unwrap(),
            // We can unwrap here because every Keycloak group has got a name.
            name: group_representation.name.unwrap(),
            // We can unwrap here because every Keycloak group has got a path.
            path: group_representation.path.unwrap(),
            attributes: group_representation.attributes.unwrap_or_default(),
        }
    }

    pub fn new_with_parent(
        keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi>,
        group_representation: keycloak::types::GroupRepresentation,
        parent: std::sync::Arc<KeycloakGroup>,
    ) -> KeycloakGroup {
        let mut group = Self::new_from_group_representation(keycloak_api, group_representation);
        group.root_group = match &parent.root_group {
            Some(root) => Some(root.clone()),
            None => Some(parent.clone()),
        };
        group.parent_group = Some(parent);
        group
    }

    pub fn from_webhook_group(keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi>, webhook_group: KeycloakWebhookGroup) -> Self {
        Self {
            keycloak_api,
            parent_group: None,
            root_group: None,
            id: webhook_group.id,
            name: webhook_group.name,
            path: webhook_group.path,
            attributes: webhook_group.attributes,
        }
    }
}

/// Resolves `group_representation` together with all of its ancestors, by walking upwards via each group's `parent_id`
/// until a root group is reached. Every group resolved this way is inserted into `cache`, keyed by ID.
/// Passing the same `cache` into multiple calls therefore only fetches ancestors shared
/// between them once, and `cache` ends up holding the union of all resolved groups across those calls.
pub async fn resolve_group_with_ancestors(
    keycloak_api: std::sync::Arc<dyn crate::external::KeycloakApi>,
    group_representation: keycloak::types::GroupRepresentation,
    cache: &mut std::collections::HashMap<String, std::sync::Arc<KeycloakGroup>>,
) -> Result<(), KidsError> {
    // We can unwrap here because every Keycloak group has an ID.
    if cache.contains_key(group_representation.id.as_ref().unwrap()) {
        return Ok(());
    }

    // Walk upwards from `group_representation`, collecting representations until we reach a root group or an already-cached ancestor.
    let mut unresolved_chain = vec![group_representation];
    while let Some(parent_id) = unresolved_chain.last().unwrap().parent_id.as_ref().filter(|id| !cache.contains_key(*id)) {
        unresolved_chain.push(keycloak_api.get_group(parent_id).await?);
    }

    // Build root to leaf, since `new_with_parent` needs the parent to already be constructed.
    let mut parent = unresolved_chain
        .last()
        .unwrap()
        .parent_id
        .as_ref()
        .and_then(|parent_id| cache.get(parent_id).cloned());
    for representation in unresolved_chain.into_iter().rev() {
        // We can unwrap here because every Keycloak group has an ID.
        let id = representation.id.clone().unwrap();
        let group_instance = match parent {
            Some(parent) => std::sync::Arc::new(KeycloakGroup::new_with_parent(keycloak_api.clone(), representation, parent)),
            None => std::sync::Arc::new(KeycloakGroup::new_from_group_representation(keycloak_api.clone(), representation)),
        };
        cache.insert(id, group_instance.clone());
        parent = Some(group_instance);
    }

    Ok(())
}

#[async_trait::async_trait]
impl kids_lib::interface::source::Group for KeycloakGroup {
    fn id(&self) -> &kids_lib::types::SharedResourceIdentifier {
        // We can unwrap here because every Keycloak group has got an ID.
        &self.id
    }

    fn name(&self) -> &str {
        // We can unwrap here because every Keycloak group has got a name.
        &self.name
    }

    fn path(&self) -> &str {
        // We can unwrap here because every Keycloak group has got a path.
        &self.path
    }

    fn attributes(&self) -> &HashMap<String, Vec<String>> {
        &self.attributes
    }

    fn root_group(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn kids_lib::interface::source::Group> {
        match &self.root_group {
            Some(root_group) => root_group.clone(),
            None => self,
        }
    }

    fn parent_group(&self) -> Option<std::sync::Arc<dyn kids_lib::interface::source::Group>> {
        match &self.parent_group {
            Some(parent_group) => Some(parent_group.clone()),
            None => None,
        }
    }

    async fn sub_groups(self: std::sync::Arc<Self>) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::Group>>, KidsError> {
        let sub_groups = self.keycloak_api.get_subgroups(self.id()).await?;
        Ok(sub_groups
            .into_iter()
            .map(|g| {
                std::sync::Arc::new(KeycloakGroup::new_with_parent(self.keycloak_api.clone(), g, self.clone()))
                    as std::sync::Arc<dyn kids_lib::interface::source::Group>
            })
            .collect())
    }

    async fn users(
        &self,
        include_subgroup_users: bool,
    ) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>>, kids_lib::error::KidsError> {
        let direct_members = self
            .keycloak_api
            .get_users_of_group(&self.id)
            .await?
            .into_iter()
            .map(|user| {
                std::sync::Arc::new(crate::user::KeycloakUser::from_user_representation(self.keycloak_api.clone(), user))
                    as std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>
            })
            .collect();
        if !include_subgroup_users {
            return Ok(direct_members);
        }
        let all_subgroup_ids = {
            let mut all_subgroup_ids = self
                .keycloak_api
                .get_subgroups(&self.id)
                .await?
                .into_iter()
                .filter_map(|group| group.id)
                .collect::<std::collections::HashSet<_>>();
            let mut subgroups_to_handle = all_subgroup_ids.clone().into_iter().collect::<Vec<_>>();
            while let Some(subgroup_id) = subgroups_to_handle.pop() {
                let subgroups = self
                    .keycloak_api
                    .get_subgroups(subgroup_id.as_str())
                    .await?
                    .into_iter()
                    .filter_map(|group| group.id)
                    .collect::<Vec<_>>();
                all_subgroup_ids.extend(subgroups.clone());
                subgroups_to_handle.extend(subgroups);
            }
            all_subgroup_ids
        };
        let mut members = direct_members;
        for subgroup_id in all_subgroup_ids {
            let direct_members = self
                .keycloak_api
                .get_users_of_group(&subgroup_id)
                .await?
                .into_iter()
                .map(|user| {
                    std::sync::Arc::new(crate::user::KeycloakUser::from_user_representation(self.keycloak_api.clone(), user))
                        as std::sync::Arc<dyn kids_lib::interface::source::User + Send + Sync>
                })
                .collect::<Vec<_>>();
            members.extend(direct_members);
        }
        Ok(members)
    }
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct KeycloakWebhookGroup {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub path: String,
    pub attributes: std::collections::HashMap<String, Vec<String>>,
}

#[cfg(test)]
mod test {
    use super::*;
    use kids_test_lib::util::constants;
    use mockall::predicate;
    #[tokio::test]
    async fn test_sub_groups() {
        // given
        let mut mock = crate::external::MockKeycloakApi::new();
        mock.expect_get_subgroups()
            .with(predicate::eq(constants::DEFAULT_SOURCE_GROUP_ID))
            .returning(|_| {
                Ok(vec![
                    crate::external::test::KeycloakGroupRepresentationBuilder::default()
                        .id(constants::ANOTHER_SOURCE_GROUP_ID)
                        .build_into(),
                ])
            });

        let group = std::sync::Arc::new(KeycloakGroup::new_from_group_representation(
            std::sync::Arc::new(mock),
            crate::external::test::KeycloakGroupRepresentationBuilder::default()
                .id(constants::DEFAULT_SOURCE_GROUP_ID)
                .build_into(),
        )) as std::sync::Arc<dyn kids_lib::interface::source::Group>;

        // when
        let sub_groups = group.clone().sub_groups().await.unwrap();

        // then
        assert_eq!(sub_groups.len(), 1);
        assert_eq!(sub_groups[0].id(), constants::ANOTHER_SOURCE_GROUP_ID);
        // also assigns parent relationships
        assert!(std::sync::Arc::ptr_eq(&sub_groups[0].parent_group().unwrap(), &group));
        assert!(std::sync::Arc::ptr_eq(&sub_groups[0].clone().root_group(), &group));
    }
}
