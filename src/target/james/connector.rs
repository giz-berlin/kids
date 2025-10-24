use crate::target::interface;
use crate::{error, source, types};
use std::{collections, rc};
use crate::target::james::dto;
use crate::target::james::external;

#[derive(serde::Deserialize)]
pub struct JamesConfig {
    pub james_api: external::JamesApiConfig,
    pub source_james_group_attr: String,
    pub source_james_team_attr: String,
    pub source_james_alias_attr: String,
}

const USER_INBOX_NAME: &str = "INBOX";

pub struct Connector {
    config: JamesConfig,
    james_api: Box<dyn external::JamesApi>,
    group_id_mapping: collections::HashMap<types::SharedResourceIdentifier, dto::Group>,
    user_id_mapping: collections::HashMap<types::SharedResourceIdentifier, dto::User>,
    james_domains: Vec<String>,
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
            group_id_mapping: collections::HashMap::new(),
            user_id_mapping: collections::HashMap::new(),
            james_domains: vec![],
        })
    }

    fn info(&self) -> String {
        "James Connector!".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), error::KidsError> {
        tracing::info!("Prepare full sync...");
        self.group_id_mapping.clear();
        self.user_id_mapping.clear();
        self.james_domains.clear();

        self.james_domains = self.james_api.list_domains().await?;

        let james_groups= self
            .james_api
            .list_groups()
            .await?;

        let james_teams = self
            .james_api
            .list_teams()
            .await?;

        for group in james_groups.into_iter() {
            let source_group_id = self.get_source_id(&group);
            let new_group = dto::Group {
                email: group,
                has_group: true,
                has_team: false,
            };
            if self.group_id_mapping.contains_key(&source_group_id) {
                // TODO: Warning
            }
            self.group_id_mapping.insert(source_group_id, new_group);
        }

        for team in james_teams.into_iter() {
            let source_group_id = team.name.to_string();

            // TODO: did we need warning? teams shouldn't be double
            if self.group_id_mapping.contains_key(&source_group_id) {
                self.group_id_mapping.get_mut(&source_group_id).unwrap().has_team = true;
                continue
            }

            let new_team = dto::Group {
                email: team.emailAddress,
                has_group: false,
                has_team: true,
            };
            self.group_id_mapping.insert(source_group_id, new_team);
        }

        tracing::info!("Groups {:?}", self.group_id_mapping);

        let james_users = self
            .james_api
            .list_users()
            .await?;

        for user in james_users.into_iter() {
            let source_user_id = self.get_source_id(&user.username);
            if self.user_id_mapping.contains_key(&source_user_id) {
                // TODO: Warning
            }
            self.user_id_mapping.insert(source_user_id, user);
        }

        tracing::info!("User: {:?}", self.user_id_mapping);

        tracing::info!("Full sync done");

        Ok(())
    }

    async fn all_groups(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.group_id_mapping.keys().cloned().collect())
    }

    async fn all_users(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.user_id_mapping.keys().cloned().collect())
    }

    async fn delete_group(&mut self, _group_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        if !self.group_id_mapping.contains_key(&_group_id) {
            tracing::warn!(
                _group_id,
                "Source group has no known associated team or group in James that could be deleted. Nothing to be done"
            );
            return Ok(());
        }

        let has_team = self.group_id_mapping.get(&_group_id).unwrap().has_team;
        let has_group = self.group_id_mapping.get(&_group_id).unwrap().has_group;
        let group_uuid_email = self.create_uuid_group_email(&_group_id);

        if has_team {
            self.delete_all_aliases_and_member_from_team(&_group_id, &group_uuid_email, &vec![]).await?;
            match self.james_api.delete_team(&_group_id).await {
                Ok(_) => {
                    tracing::info!(group_id = _group_id, "Delete team")
                }
                Err(e) => {
                    tracing::warn!(group_id = _group_id, "Failed to delete team: {e}")
                }
            };
        }

        if has_group {
            self.delete_all_aliases_and_member_from_group(&group_uuid_email, &vec![]).await?;
            tracing::info!(_group_id, "Delete group");
        }

        self.group_id_mapping.remove(&_group_id);


        Ok(())
    }

    async fn delete_user(&mut self, _user_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        if !self.user_id_mapping.contains_key(&_user_id) {
            tracing::warn!(source_user_id = _user_id, "Cannot deactivate source user, because it is not known to James");
        }

        let uuid_email = self.create_uuid_user_email(&_user_id);
        self.update_alias(&uuid_email, &vec![]).await?;
        self.james_api.delete_mailbox(&uuid_email, USER_INBOX_NAME).await?;
        self.james_api.delete_user(&uuid_email).await?;
        self.user_id_mapping.remove(&_user_id);

        Ok(())
    }

    async fn create_or_update_group(&mut self, source_group: std::sync::Arc<Box<dyn source::interface::Group + Sync + Send>>) -> Result<(), error::KidsError> {
        let source_group_id = source_group.id();
        let has_groups_in_source = source_group.attributes().contains_key(&self.config.source_james_group_attr);
        let has_teams_in_source = source_group.attributes().contains_key(&self.config.source_james_team_attr);
        let group_uuid_email = self.create_uuid_group_email(source_group_id);

        if !self.group_id_mapping.contains_key(source_group_id) {
            if !has_groups_in_source && !has_teams_in_source {
                tracing::info!(source_group_id, "Skipping group, because it has no james-attribute set and it was not in group mapping");
                return Ok(());
            };

            let new_group = dto::Group {
                email: group_uuid_email.clone(),
                has_group: has_groups_in_source,
                has_team: has_teams_in_source,
            };
            self.group_id_mapping.insert(source_group_id.to_string(), new_group);
        }

        let group_info = self.group_id_mapping.get(source_group_id).unwrap();
        let has_groups_in_james = group_info.has_group;
        let has_teams_in_james = group_info.has_team;

        let all_teams = self.james_api.list_teams().await?;
        let team_exists = all_teams.contains(&dto::Team{ name: source_group_id.clone(), emailAddress: group_uuid_email.clone() });

        // no need for creating james groups because they are created with adding first or deleting last user
        if has_teams_in_source && !team_exists {
            // we cannot create a james team if a group exist with the same address exist
            // so we need to empty the group here, in order to create the team
            // group would be created again with adding users in create_or_update_user
            if has_groups_in_james {
                self.delete_all_member_from_group(&group_uuid_email).await?;
            }
            self.james_api.create_team(source_group_id).await?;
            tracing::info!(source_group_id, "New team created.");
        }

        let mut desired_aliases: Vec<String> = vec![];
        let mut team_aliases: &Vec<String> = &vec![];
        let mut group_aliases: &Vec<String> = &vec![];
        if has_teams_in_source {
            team_aliases = source_group.attributes().get(&self.config.source_james_team_attr).unwrap();
            team_aliases.iter().for_each(|attr| desired_aliases.push(attr.clone()));
        }
        if has_groups_in_source {
            group_aliases = source_group.attributes().get(&self.config.source_james_group_attr).unwrap();
            group_aliases.iter().for_each(|attr| desired_aliases.push(attr.clone()));
        }
        self.update_alias(&group_uuid_email, &desired_aliases).await?;


        // If james-team attribut is removed, remove all aliases and delete all users from the team
        if !has_teams_in_source && has_teams_in_james {
            self.delete_all_aliases_and_member_from_team(source_group_id, &group_uuid_email, group_aliases).await?;
        }

        // If james-mailing-list-receiver attribut is removed, remove all aliases and delete all users from the group
        if !has_groups_in_source && has_groups_in_james {
            self.delete_all_aliases_and_member_from_group(&group_uuid_email, team_aliases).await?;
        }
        Ok(())
    }

    async fn create_or_update_user(&mut self, source_user: std::sync::Arc<Box<dyn source::interface::User + Sync + Send>>) -> Result<(), error::KidsError> {
        let user_uuid_email = self.create_uuid_user_email(source_user.id());

        if !self.user_id_mapping.contains_key(source_user.id()) {
            // TODO: Where I get the password? --> could be empty because it shouldn't be used --> test it over roundcube

            self.james_api.create_user(&user_uuid_email, "").await?;
            tracing::info!(source_user_id = source_user.id(), "New user created");
            self.james_api.create_mailbox(&user_uuid_email, USER_INBOX_NAME).await?;
            tracing::info!(source_user_id = source_user.id(), "Inbox created");
            self.user_id_mapping.insert(source_user.id().to_owned(), dto::User { username: user_uuid_email.clone() });
        }

        let mut desired_aliases = &vec![];
        if source_user.attributes().contains_key(self.config.source_james_alias_attr.as_str()) {
            desired_aliases = source_user.attributes().get(self.config.source_james_alias_attr.as_str()).unwrap();
        }

        self.update_alias(&user_uuid_email, &desired_aliases).await?;

        // Create email as alias iif domain exists in james
        if let Some(email) = source_user.email() {
            let domain = self.get_domain_from(email);
            let current_aliases: Vec<String> = self.james_api
                .get_aliases_of(&user_uuid_email)
                .await?
                .iter()
                .map(|alias| alias.source.clone())
                .collect();
            if !current_aliases.contains(&email.to_string()) && self.is_domain_in_james_domain(&domain) {
                self.james_api.add_alias(&user_uuid_email, email).await?;
            }
        }

        let desired_source_groups = source_user.groups().await.map_err(
            |e| e.with_context(&format!("Could not get source groups associated with source user {}", source_user.id()))
        )?;

        // update james teams
        let desired_team_ids: Vec<types::SharedResourceIdentifier> = desired_source_groups
            .iter()
            .filter(|group| group.attributes().contains_key(&self.config.source_james_team_attr))
            .map(|group| group.id().to_owned())
            .collect();

        // TODO: should we just log an error and try to continue with groups instead of returning?
        let current_teams = self.james_api.list_user_teams(&user_uuid_email).await.map_err(
            |e| e.with_context(&format!("Could not get current james teams for user {}", source_user.id())),
        )?;
        let current_team_ids: Vec<types::SharedResourceIdentifier> = current_teams.into_iter().map(|team| team.name).collect();

        for team_id in desired_team_ids.iter() {
            if !current_team_ids.contains(team_id) {
                // TODO: What is with roles
                match self.james_api.add_member_to_team(team_id, &user_uuid_email, "member").await {
                    Ok(_) => tracing::info!(source_user_id = source_user.id(), team_id, "Add user to team"),
                    Err(e) => tracing::warn!(?e, source_user_id = source_user.id(), team_id, "Could not add user to team"),
                }
            }
        }

        for team_id in current_team_ids.iter() {
            if !desired_team_ids.contains(team_id) {
                match self.james_api.remove_member_from_team(team_id, &user_uuid_email).await {
                    Ok(_) => tracing::info!(source_user_id = source_user.id(), team_id, "Remove user from team"),
                    Err(e) => tracing::warn!(?e, source_user_id = source_user.id(), team_id, "Could not remove user from team"),
                }
            }
        }

        // update james groups
        let desired_groups: Vec<String> = desired_source_groups
            .iter()
            .filter(|group| group.attributes().contains_key(&self.config.source_james_group_attr))
            .map(|group| self.create_uuid_group_email(group.id()))
            .collect();

        // TODO: should we just log an error and try to continue with groups instead of returning?
        let all_james_groups = self.james_api.list_groups().await.map_err(
            |e| e.with_context("Could not get all james groups"),
        )?;
        let mut current_groups: Vec<String> = vec![];

        for group_email in all_james_groups.iter() {
            match self.james_api.list_group_members(group_email).await {
                Ok(members) => {
                    if members.contains(&user_uuid_email) {
                        current_groups.push(group_email.clone());
                    }
                }
                Err(e) => {
                    tracing::warn!(?e, group_id = self.get_source_id(group_email), "Could not get group members for group to get current user groups")
                }
            };
        }

        for group_email in desired_groups.iter() {
            if !current_groups.contains(group_email) {
                match self.james_api.add_member_to_group(group_email, &user_uuid_email).await {
                    Ok(_) => tracing::info!(source_user_id = source_user.id(), group_id = self.get_source_id(group_email), "Add user to group"),
                    Err(e) => tracing::warn!(?e, source_user_id = source_user.id(), group_id = self.get_source_id(group_email), "Could not add user to group")
                }
            }
        }

        for group_email in current_groups.iter() {
            if !desired_groups.contains(group_email) {
                match self.james_api.remove_member_from_group(group_email, &user_uuid_email).await {
                    Ok(_) => tracing::info!(source_user_id = source_user.id(), group_id = self.get_source_id(group_email), "Remove user from group"),
                    Err(e) => tracing::warn!(?e, source_user_id = source_user.id(), group_id = self.get_source_id(group_email), "Could not remove user from group")
                }
            }
        }

        Ok(())
    }
}

impl Connector {
    fn create_uuid_user_email(&self, source_user_id: &str) -> String {
        source_user_id.to_string() + "@" + self.config.james_api.initial_user_domain.as_str()
    }
    fn create_uuid_group_email(&self, source_group_id: &str) -> String {
        source_group_id.to_string() + "@" + self.config.james_api.initial_group_domain.as_str()
    }
    fn get_source_id(&self, username: &str) -> String {
        username.split("@").collect::<Vec<&str>>()[0].to_string()
    }

    fn get_domain_from(&self, email: &str) -> String {
        email.split("@").collect::<Vec<&str>>()[1].to_string()
    }

    fn is_domain_in_james_domain(&self, domain: &String) -> bool {
        self.james_domains.contains(domain)
    }

    async fn update_alias(&mut self, uuid_email: &str, desired_aliases: &Vec<String>) -> Result<(), error::KidsError> {
        // TODO: For testing you need to enable Unmanaged Attributes in Realm Settings in Keycloak (need to update Keycloak Config)
        let current_aliases: Vec<String> = self.james_api
            .get_aliases_of(uuid_email)
            .await?
            .iter()
            .map(|alias| alias.source.clone())
            .collect();
        tracing::info!(uuid_email, ?current_aliases);
        tracing::info!(uuid_email, ?desired_aliases);

        for alias in desired_aliases.iter() {
            let domain =  self.get_domain_from(alias);
            if !self.is_domain_in_james_domain(&domain) {
                tracing::warn!(alias, domain = ?self.james_domains,  "Domain of alias not contained in james domains");
                continue;
            }
            if !current_aliases.contains(alias)  {
                self.james_api.add_alias(uuid_email, &alias).await.map_err(
                    |e| e.with_context("Could not add alias for user")
                )?;
            }
        }

        for alias in current_aliases.iter() {
            let domain =  self.get_domain_from(alias);
            if !desired_aliases.contains(alias) && self.is_domain_in_james_domain(&domain) {
                self.james_api.remove_alias(uuid_email, alias).await.map_err(
                    |e| e.with_context("Could not delete alias for user")
                )?;
            }
        }

        tracing::info!(uuid_email, "Aliases updated");

        Ok(())
    }

    async fn delete_all_aliases_and_member_from_team(&mut self, source_group_id: &str, group_uuid_email: &str, group_aliases: &Vec<String>) -> Result<(), error::KidsError> {
        self.update_alias(group_uuid_email, group_aliases).await?;
        let response: Vec<dto::Member> = self.james_api.list_team_members(source_group_id).await?;
        let team_members = response;
        for member in team_members.into_iter() {
            self.james_api.remove_member_from_team(source_group_id, &member.username).await?;
        }

        Ok(())
    }

    async fn delete_all_aliases_and_member_from_group(&mut self, group_uuid_email: &str, team_aliases: &Vec<String>) -> Result<(), error::KidsError> {
        self.update_alias(&group_uuid_email, team_aliases).await?;
        self.delete_all_member_from_group(group_uuid_email).await?;

        Ok(())
    }

    async fn delete_all_member_from_group(&mut self, group_uuid_email: &str) -> Result<(), error::KidsError> {
        let members = match self.james_api.list_group_members(group_uuid_email).await {
            Ok(members) => {members}
            Err(_) => {
                tracing::info!("Group has no members, so no need vor delete them");
                return Ok(())
            }
        };
        for member in members.into_iter() {
            self.james_api.remove_member_from_group(group_uuid_email, &member).await?;
        }

        Ok(())
    }
}
