use crate::target::interface;
use crate::target::james::dto;
use crate::target::james::external;
use crate::{error, source, types};
use anyhow::anyhow;
use std::collections;

#[derive(serde::Deserialize)]
pub struct JamesConfig {
    pub james_api: external::JamesApiConfig,
    /// Attribute names in source groups and users corresponding to teams, groups and users in James.
    /// Only source groups that have the James team or group attribute will be synced to James.
    pub source_james_list_attr: String,
    pub source_james_team_attr: String,
    pub source_james_alias_attr: String,
}

/// A connector to James providing the [Target](interface::Target) interface.
pub struct Connector {
    config: JamesConfig,
    james_api: Box<dyn external::JamesApi + Send + Sync>,
    group_id_mapping: Option<collections::HashMap<types::SharedResourceIdentifier, dto::Group>>,
    user_ids: Option<collections::HashSet<types::SharedResourceIdentifier>>,
    /// Domains that are set up in James. Only team, list, user and alias addresses with these domains will be created.
    /// If an address has a domain not in this array, we will not create it.
    james_domains: Option<Vec<String>>,
}

#[async_trait::async_trait]
impl interface::Target for Connector {
    type Config = JamesConfig;

    async fn new(config: Self::Config) -> Result<Self, error::KidsError> {
        let james_api = Box::new(
            external::JamesClient::new(config.james_api.clone())
                .await
                .map_err(|e| e.with_context("Failed to create James API client"))?,
        );
        Ok(Connector {
            config,
            james_api,
            group_id_mapping: None,
            user_ids: None,
            james_domains: None,
        })
    }

    fn info(&self) -> String {
        "James Connector!".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), error::KidsError> {
        tracing::info!(
            "To prepare for full sync, rebuilding mapping between source group IDs and James team/list IDs, as well as source user IDs and James user IDs"
        );
        self.group_id_mapping.take();
        self.user_ids.take();
        self.james_domains.take();
        Ok(())
    }

    async fn all_groups(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.get_cached_group_id_mapping().await?.keys().cloned().collect())
    }

    async fn all_users(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.get_cached_user_ids().await?.iter().cloned().collect())
    }

    async fn delete_group(&mut self, _group_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        if !self.get_cached_group_id_mapping().await?.contains_key(&_group_id) {
            tracing::warn!(
                _group_id,
                "Source group has no known associated team or list in James that could be deleted. Nothing to be done"
            );
            return Ok(());
        }

        let has_team;
        let has_list;
        if let Some(group) = self.get_cached_group_id_mapping().await?.get(&_group_id) {
            has_team = group.has_team;
            has_list = group.has_list;
        } else {
            return Err(error::KidsError::InternalError(
                "Source group should be in group id mapping due to previous check, but we cannot get it".to_string(),
            ));
        }

        let uuid_team_email = self
            .create_uuid_team_email(&_group_id)
            .map_err(|error| error.with_context(&format!("group_id = {}, Could not delete group", _group_id)))?;

        if has_team {
            self.delete_all_aliases_and_members_from_team(&_group_id, &uuid_team_email, &[]).await?;
            match self.james_api.delete_team(&_group_id).await {
                Ok(_) => tracing::info!(group_id = _group_id, "Delete team"),
                Err(error) => return Err(error.with_context(&format!("group_id = {}, Could not delete team", _group_id))),
            };
        }

        let uuid_list_email = self
            .create_uuid_list_email(&_group_id)
            .map_err(|error| error.with_context(&format!("group_id = {}, Could not delete group", _group_id)))?;

        if has_list {
            self.delete_all_aliases_and_members_from_list(&uuid_list_email, &[]).await?;
            tracing::info!(_group_id, "Delete list");
        }

        self.get_cached_group_id_mapping().await?.remove(&_group_id);

        Ok(())
    }

    async fn delete_user(&mut self, _user_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        if !self.get_cached_user_ids().await?.contains(&_user_id) {
            tracing::warn!(user_id = _user_id, "Cannot deactivate source user, because it is not known to James");
        }

        let uuid_user_email = self
            .create_uuid_user_email(&_user_id)
            .map_err(|error| error.with_context(&format!("user_id = {}, Could not delete user", _user_id)))?;
        self.update_alias(&uuid_user_email, &[]).await?;
        match self.james_api.delete_user(&uuid_user_email).await {
            Ok(_) => tracing::info!(user_id = _user_id, "Delete user"),
            Err(error) => return Err(error.with_context(&format!("user_id = {}, Could not delete user", _user_id))),
        };

        self.get_cached_user_ids().await?.remove(&_user_id);

        Ok(())
    }

    async fn create_or_update_group(&mut self, source_group: std::sync::Arc<dyn source::interface::Group + Sync + Send>) -> Result<(), error::KidsError> {
        let source_group_id = source_group.id();
        let has_lists_in_source = source_group.attributes().contains_key(&self.config.source_james_list_attr);
        let has_teams_in_source = source_group.attributes().contains_key(&self.config.source_james_team_attr);

        if !self.get_cached_group_id_mapping().await?.contains_key(source_group_id) {
            if !has_lists_in_source && !has_teams_in_source {
                tracing::info!(
                    source_group_id,
                    "Skipping group, because it has no James attribute set and it was not in group mapping"
                );
                return Ok(());
            };

            let new_group = dto::Group {
                has_list: has_lists_in_source,
                has_team: has_teams_in_source,
            };
            self.get_cached_group_id_mapping().await?.insert(source_group_id.to_string(), new_group);
        }

        let has_lists_in_james;
        let has_teams_in_james;
        if let Some(group) = self.get_cached_group_id_mapping().await?.get(source_group_id) {
            has_teams_in_james = group.has_team;
            has_lists_in_james = group.has_list;
        } else {
            return Err(error::KidsError::InternalError(
                "Source group should be in group id mapping due to previous check, but we cannot get it".to_string(),
            ));
        }

        let team_uuid_email = self
            .create_uuid_team_email(source_group_id)
            .map_err(|error| error.with_context(&format!("group_id = {}, Could not update teams and lists for group", source_group_id)))?;
        let list_uuid_email = self
            .create_uuid_list_email(source_group_id)
            .map_err(|error| error.with_context(&format!("group_id = {}, Could not update teams and lists for group", source_group_id)))?;

        let all_teams = self.james_api.get_teams().await?;
        let team_exists = all_teams.contains(&dto::Team {
            id: source_group_id.clone(),
            email_address: team_uuid_email.clone(),
        });

        // no need for creating james lists because they are created with adding first or deleting last user
        if has_teams_in_source && !team_exists {
            match self.james_api.create_team(source_group_id).await {
                Ok(_) => tracing::info!(source_group_id, "Create new team"),
                Err(error) => return Err(error.with_context(&format!("group_id = {}, Could not create new team for", source_group_id))),
            };
        }

        let mut team_aliases: &Vec<String> = &vec![];
        let mut list_aliases: &Vec<String> = &vec![];
        if has_teams_in_source {
            if let Some(aliases) = source_group.attributes().get(&self.config.source_james_team_attr) {
                team_aliases = aliases
            }
        }
        self.update_alias(&team_uuid_email, team_aliases).await?;

        if has_lists_in_source {
            if let Some(aliases) = source_group.attributes().get(&self.config.source_james_list_attr) {
                list_aliases = aliases
            }
        }
        self.update_alias(&list_uuid_email, list_aliases).await?;

        // If james-team attribut is removed, remove all aliases and delete all users from the team
        if !has_teams_in_source && has_teams_in_james {
            self.delete_all_aliases_and_members_from_team(source_group_id, &team_uuid_email, &[]).await?;
        }

        // If james-list attribut is removed, remove all aliases and delete all users from the list
        if !has_lists_in_source && has_lists_in_james {
            self.delete_all_aliases_and_members_from_list(&list_uuid_email, &[]).await?;
        }
        Ok(())
    }

    async fn create_or_update_user(&mut self, source_user: std::sync::Arc<dyn source::interface::User + Sync + Send>) -> Result<(), error::KidsError> {
        let user_uuid_email = self
            .create_uuid_user_email(source_user.id())
            .map_err(|error| error.with_context(&format!("user_id = {}, Could not create or update user", source_user.id())))?;

        if !self.get_cached_user_ids().await?.contains(source_user.id()) {
            match self.james_api.create_user(&user_uuid_email).await {
                Ok(_) => tracing::info!(user_id = source_user.id(), "Create new user"),
                Err(error) => return Err(error.with_context(&format!("user_id = {}, Could not create new user", source_user.id()))),
            };
            self.get_cached_user_ids().await?.insert(source_user.id().clone());
        }

        let mut desired_aliases: Vec<String> = vec![];

        if let Some(attributes) = source_user.attributes().get(self.config.source_james_alias_attr.as_str()) {
            desired_aliases = attributes.clone();
        }
        if let Some(email) = source_user.email() {
            if !desired_aliases.contains(&email.to_string()) {
                desired_aliases.push(email.to_string());
            }
        }
        self.update_alias(&user_uuid_email, &desired_aliases).await?;

        let desired_source_groups = source_user.groups().await.map_err(|e| {
            e.with_context(&format!(
                "user_id = {}, Could not get source groups associated with source user",
                source_user.id()
            ))
        })?;

        // update James teams
        let desired_team_ids: Vec<types::SharedResourceIdentifier> = desired_source_groups
            .iter()
            .filter(|group| group.attributes().contains_key(&self.config.source_james_team_attr))
            .map(|group| group.id().to_owned())
            .collect();

        let current_teams = self
            .james_api
            .get_user_teams(&user_uuid_email)
            .await
            .map_err(|e| e.with_context(&format!("user_id = {}, Could not get current james teams for user", source_user.id())))?;
        let current_team_ids: Vec<types::SharedResourceIdentifier> = current_teams.into_iter().map(|team| team.id).collect();

        for team_id in desired_team_ids.iter() {
            if !current_team_ids.contains(team_id) {
                match self.james_api.add_member_to_team(team_id, &user_uuid_email).await {
                    Ok(_) => tracing::info!(source_user_id = source_user.id(), team_id, "Add user to team"),
                    Err(error) => tracing::error!(?error, source_user_id = source_user.id(), team_id, "Could not add user to team"),
                }
            }
        }

        for team_id in current_team_ids.iter() {
            if !desired_team_ids.contains(team_id) {
                match self.james_api.remove_member_from_team(team_id, &user_uuid_email).await {
                    Ok(_) => tracing::info!(source_user_id = source_user.id(), team_id, "Remove user from team"),
                    Err(error) => tracing::error!(?error, source_user_id = source_user.id(), team_id, "Could not remove user from team"),
                }
            }
        }

        // update James lists
        let desired_list_ids: Vec<String> = desired_source_groups
            .iter()
            .filter(|group| group.attributes().contains_key(&self.config.source_james_list_attr))
            .map(|group| group.id().to_string())
            .collect();

        let mut desired_lists: Vec<String> = vec![];
        for id in desired_list_ids.iter() {
            let email = match self.create_uuid_list_email(id) {
                Ok(email) => Some(email),
                Err(error) => {
                    tracing::error!(?error, source_user_id = source_user.id(), list_id = id, "Could not update list for user");
                    None
                }
            };
            if let Some(email) = email {
                desired_lists.push(email);
            }
        }

        let all_james_lists = self.james_api.get_lists().await.map_err(|e| e.with_context("Could not get all james lists"))?;
        // We also save the member count to decide if we are the last member of list
        let mut current_lists_member_count_mapping: collections::HashMap<String, usize> = collections::HashMap::new();

        for list_email in all_james_lists.iter() {
            match self.james_api.get_list_members(list_email).await {
                Ok(members) => {
                    if members.contains(&user_uuid_email) {
                        current_lists_member_count_mapping.insert(list_email.clone(), members.len());
                    }
                }
                Err(error) => {
                    tracing::error!(
                        ?error,
                        group_id = self.get_local_part_from(list_email),
                        "Could not get members for list to get current user lists"
                    )
                }
            };
        }

        for list_email in desired_lists.iter() {
            if !current_lists_member_count_mapping.contains_key(list_email) {
                match self.james_api.add_member_to_list(list_email, &user_uuid_email).await {
                    Ok(_) => tracing::info!(
                        source_user_id = source_user.id(),
                        group_id = self.get_local_part_from(list_email),
                        "Add user to list"
                    ),
                    Err(error) => tracing::error!(
                        ?error,
                        source_user_id = source_user.id(),
                        group_id = self.get_local_part_from(list_email),
                        "Could not add user to list"
                    ),
                }
            }
        }

        for (list_email, member_count) in current_lists_member_count_mapping.iter() {
            if !desired_lists.contains(list_email) {
                match self.james_api.remove_member_from_list(list_email, &user_uuid_email).await {
                    Ok(_) => {
                        // If this user was the last one of the list, the list is automatically deleted. But aliases will remain.
                        // We cannot delete the aliases of a list in the delete group method in a later sync run
                        // because the list will not be part of the group_id_mapping anymore.
                        if *member_count == 1 {
                            self.update_alias(list_email, &[]).await?;
                        }
                        tracing::info!(
                            source_user_id = source_user.id(),
                            group_id = self.get_local_part_from(list_email),
                            "Remove user from list"
                        )
                    }
                    Err(error) => tracing::error!(
                        ?error,
                        source_user_id = source_user.id(),
                        group_id = self.get_local_part_from(list_email),
                        "Could not remove user from list"
                    ),
                }
            }
        }

        Ok(())
    }
}

impl Connector {
    fn is_valid_id(&self, source_user_id: &str) -> bool {
        source_user_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }
    fn is_valid_domain(&self, domain: &str) -> bool {
        domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    }
    fn is_valid_local_part(&self, local_part: &str) -> bool {
        local_part.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || c == '-'
                || c == '.'
                || c == '!'
                || c == '#'
                || c == '$'
                || c == '%'
                || c == '&'
                || c == '\''
                || c == '*'
                || c == '+'
                || c == '/'
                || c == '='
                || c == '?'
                || c == '^'
                || c == '_'
                || c == '`'
                || c == '{'
                || c == '}'
                || c == '|'
                || c == '~'
        })
    }
    fn create_uuid_user_email(&self, source_user_id: &str) -> Result<String, error::KidsError> {
        if !(self.is_valid_id(source_user_id) && self.is_valid_domain(self.config.james_api.james_user_domain.as_str())) {
            return Err(error::KidsError::RequestFailed(
                "".to_string(),
                anyhow!(format!(
                    "uuid email address for user has invalid format because of wrong uuid ({}) or domain format ({})",
                    source_user_id, self.config.james_api.james_user_domain
                )),
            ));
        }
        Ok(source_user_id.to_string() + "@" + self.config.james_api.james_user_domain.as_str())
    }
    fn create_uuid_list_email(&self, source_group_id: &str) -> Result<String, error::KidsError> {
        if !(self.is_valid_id(source_group_id) && self.is_valid_domain(self.config.james_api.james_list_domain.as_str())) {
            return Err(error::KidsError::RequestFailed(
                "".to_string(),
                anyhow!(format!(
                    "uuid email address for list has invalid format because of wrong uuid ({}) or domain format ({})",
                    source_group_id, self.config.james_api.james_list_domain
                )),
            ));
        }
        Ok(source_group_id.to_string() + "@" + self.config.james_api.james_list_domain.as_str())
    }
    fn create_uuid_team_email(&self, source_group_id: &str) -> Result<String, error::KidsError> {
        if !(self.is_valid_id(source_group_id) && self.is_valid_domain(self.config.james_api.james_team_domain.as_str())) {
            return Err(error::KidsError::RequestFailed(
                "".to_string(),
                anyhow!(format!(
                    "uuid email address for team has invalid format because of wrong uuid ({}) or domain format ({})",
                    source_group_id, self.config.james_api.james_team_domain
                )),
            ));
        }
        Ok(source_group_id.to_string() + "@" + self.config.james_api.james_team_domain.as_str())
    }
    fn get_local_part_from(&self, username: &str) -> String {
        username.split("@").collect::<Vec<&str>>()[0].to_string()
    }

    fn get_domain_from(&self, email: &str) -> String {
        email.split("@").collect::<Vec<&str>>()[1].to_string()
    }

    async fn domain_contained_in_cached_james_domains(&mut self, domain: &String) -> Result<bool, error::KidsError> {
        if self.james_domains.is_none() {
            self.james_domains = Some(self.james_api.get_domains().await.map_err(|e| e.with_context("Could not get domains"))?);
        }

        if let Some(domains) = &self.james_domains {
            return Ok(domains.contains(domain));
        }
        Err(error::KidsError::InternalError("James domains are None, which should not happen".to_string()))
    }

    async fn update_caches(&mut self) -> Result<(), error::KidsError> {
        let james_users = self.james_api.get_users().await.map_err(|e| e.with_context("Could not get James users"))?;
        self.user_ids = Some(james_users.iter().map(|user| self.get_local_part_from(&user.user_email.clone())).collect());

        let james_lists = self.james_api.get_lists().await.map_err(|e| e.with_context("Failed to get James lists"))?;
        let james_teams = self.james_api.get_teams().await.map_err(|e| e.with_context("Failed to get James teams"))?;

        let mut new_group_id_mapping = collections::HashMap::new();
        for list in james_lists.into_iter() {
            let source_group_id = self.get_local_part_from(&list);
            let new_group = dto::Group {
                has_list: true,
                has_team: false,
            };

            if new_group_id_mapping.contains_key(&source_group_id) {
                // This shouldn't be possible, because it is not possible to create 2 lists with the same address in James.
                tracing::error!(
                    source_group_id,
                    new_group = ?new_group,
                    group_in_mapping = ?new_group_id_mapping[&source_group_id],
                    "Found duplicate list in James"
                );
                continue;
            }

            new_group_id_mapping.insert(source_group_id, new_group);
        }

        for team in james_teams.into_iter() {
            let source_group_id = team.id.to_string();
            let new_team = dto::Group {
                has_list: false,
                has_team: true,
            };

            if let Some(mapping) = new_group_id_mapping.get_mut(&source_group_id) {
                if mapping.has_team {
                    tracing::error!(
                        source_group_id,
                        team_in_mapping = ?mapping,
                        "Found duplicate James team"
                    );
                }
                mapping.has_team = true;
                continue;
            }

            new_group_id_mapping.insert(source_group_id, new_team);
        }
        self.group_id_mapping = Some(new_group_id_mapping);

        Ok(())
    }

    async fn get_cached_user_ids(&mut self) -> Result<&mut collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        if self.user_ids.is_none() {
            self.update_caches().await?;
        }
        Ok(self.user_ids.as_mut().expect("User ids should be there, as we have just updated them"))
    }

    async fn get_cached_group_id_mapping(&mut self) -> Result<&mut collections::HashMap<types::SharedResourceIdentifier, dto::Group>, error::KidsError> {
        if self.group_id_mapping.is_none() {
            self.update_caches().await?;
        }
        Ok(self
            .group_id_mapping
            .as_mut()
            .expect("Group if mapping should be there, as we have just updated them"))
    }

    async fn update_alias(&mut self, uuid_email: &str, desired_aliases: &[String]) -> Result<(), error::KidsError> {
        let current_aliases: Vec<String> = match self.james_api.get_aliases_of(uuid_email).await {
            Ok(aliases) => aliases.iter().map(|alias| alias.alias_email.clone()).collect(),
            Err(error) => {
                tracing::warn!(%error, id = self.get_local_part_from(uuid_email), "Could not get aliases, assuming they have none");
                vec![]
            }
        };

        for alias in desired_aliases.iter() {
            let domain = self.get_domain_from(alias);
            let local_part = self.get_local_part_from(alias);
            if !(self.is_valid_domain(&domain) && self.is_valid_local_part(&local_part)) {
                tracing::debug!(
                    alias,
                    user_id = self.get_local_part_from(uuid_email),
                    "Alias email address has no valid format, no alias created"
                );
                continue;
            }
            if !self.domain_contained_in_cached_james_domains(&domain).await? {
                tracing::debug!(
                    alias,
                    user_id = self.get_local_part_from(uuid_email),
                    "Domain of alias not contained in James domains, no alias created"
                );
                continue;
            }
            if domain == self.config.james_api.james_user_domain
                || domain == self.config.james_api.james_team_domain
                || domain == self.config.james_api.james_list_domain
            {
                tracing::warn!(
                    alias,
                    user_id = self.get_local_part_from(uuid_email),
                    "Domain of alias should not be one of james user, list or team domain, no alias created"
                );
                continue;
            }
            if !current_aliases.contains(alias) {
                match self.james_api.add_alias(uuid_email, alias).await {
                    Ok(_) => tracing::info!(uuid_email, alias, "Add alias"),
                    Err(error) => tracing::error!(?error, uuid_email, alias, "Could not add alias"),
                }
            }
        }

        for alias in current_aliases.iter() {
            let domain = self.get_domain_from(alias);
            if !(self.domain_contained_in_cached_james_domains(&domain).await? && desired_aliases.contains(alias)) {
                match self.james_api.remove_alias(uuid_email, alias).await {
                    Ok(_) => tracing::info!(uuid_email, alias, "Delete alias"),
                    Err(error) => tracing::error!(?error, uuid_email, alias, "Could not delete alias"),
                }
            }
        }

        Ok(())
    }

    async fn delete_all_aliases_and_members_from_team(
        &mut self,
        group_id: &str,
        team_uuid_email: &str,
        team_aliases: &[String],
    ) -> Result<(), error::KidsError> {
        self.update_alias(team_uuid_email, team_aliases).await?;
        let team_members: Vec<dto::Member> = self
            .james_api
            .get_team_members(group_id)
            .await
            .map_err(|e| e.with_context(&format!("group_id = {}, Could not get team members for team", group_id)))?;
        for member in team_members.into_iter() {
            let uuid_user_email = member.user_email;
            match self.james_api.remove_member_from_team(group_id, &uuid_user_email).await {
                Ok(_) => tracing::info!(
                    user_id = self.get_local_part_from(&uuid_user_email),
                    group_id = self.get_local_part_from(team_uuid_email),
                    "Team member removed"
                ),
                Err(error) => {
                    return Err(error.with_context(&format!(
                        "user_id = {}, group_id = {}, Could not remove member from team",
                        self.get_local_part_from(&uuid_user_email),
                        self.get_local_part_from(team_uuid_email)
                    )))
                }
            };
        }
        Ok(())
    }

    async fn delete_all_aliases_and_members_from_list(&mut self, list_uuid_email: &str, list_aliases: &[String]) -> Result<(), error::KidsError> {
        self.update_alias(list_uuid_email, list_aliases).await?;
        self.delete_all_members_from_list(list_uuid_email).await?;
        Ok(())
    }

    async fn delete_all_members_from_list(&mut self, list_uuid_email: &str) -> Result<(), error::KidsError> {
        let members = match self.james_api.get_list_members(list_uuid_email).await {
            Ok(members) => members,
            Err(error) => {
                tracing::info!(%error, group_id = self.get_local_part_from(list_uuid_email), "List has no members, so no need for deleting them");
                return Ok(());
            }
        };

        for member in members.into_iter() {
            match self.james_api.remove_member_from_list(list_uuid_email, &member).await {
                Ok(_) => tracing::info!(
                    user_id = self.get_local_part_from(&member),
                    group_id = self.get_local_part_from(list_uuid_email),
                    "List member removed"
                ),
                Err(error) => {
                    return Err(error.with_context(&format!(
                        "user_id = {}, group_id = {}, Could not remove member from list",
                        self.get_local_part_from(&member),
                        self.get_local_part_from(list_uuid_email)
                    )))
                }
            };
        }
        Ok(())
    }
}
