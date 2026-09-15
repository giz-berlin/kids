mod state;

use std::collections::HashSet;

use kids_lib::error::KidsError;
use kids_lib::types::SharedResourceIdentifier;

use crate::target::external::admin::AdminClient;
use crate::target::external::cli::CliClient;
use crate::target::external::directory_api::DirectoryApiClient;
use crate::target::external::vault_api::VaultApiClient;
use crate::target::types::{Access, AccessFlags, MembershipStatus};
use state::TargetState;

pub struct Apis {
    pub admin: AdminClient,
    pub vault: VaultApiClient,
    pub directory: DirectoryApiClient,
    pub cli: CliClient,
}

pub struct VaultwardenInteractor {
    apis: Apis,
    /// Emails of accounts that are never touched, always includes the syncer's own.
    ignored_emails: HashSet<String>,
    state: TargetState,
}

impl VaultwardenInteractor {
    pub async fn new(apis: Apis, ignored_emails: impl IntoIterator<Item = String>) -> Result<Self, KidsError> {
        let mut ignored_emails: HashSet<String> = ignored_emails.into_iter().map(|email| email.to_lowercase()).collect();
        ignored_emails.insert(apis.vault.get_own_email().await?.to_lowercase());
        let state = TargetState::fetch(&apis, &ignored_emails).await?;
        let interactor = Self { apis, ignored_emails, state };
        interactor.log_state();
        Ok(interactor)
    }

    /// Discards the cached state and fetches it again.
    pub async fn refresh(&mut self) -> Result<(), KidsError> {
        self.state = TargetState::fetch(&self.apis, &self.ignored_emails).await?;
        self.log_state();
        Ok(())
    }

    fn log_state(&self) {
        tracing::info!(
            users = self.state.users.len(),
            members = self.state.members.len(),
            groups = self.state.groups.len(),
            collections = self.state.collections.len(),
            "Fetched Vaultwarden state"
        );
    }

    fn is_ignored(&self, email: &str) -> bool {
        self.ignored_emails.contains(&email.to_lowercase())
    }

    pub fn group_ids(&self) -> HashSet<SharedResourceIdentifier> {
        self.state.groups.keys().cloned().collect()
    }

    pub fn collection_ids(&self) -> HashSet<SharedResourceIdentifier> {
        self.state.collections.keys().cloned().collect()
    }

    pub fn user_ids(&self) -> HashSet<SharedResourceIdentifier> {
        self.state.members.keys().cloned().collect()
    }

    /// Ensures that a group and a collection named `name` exist for `external_id`, and that the group can access the collection.
    pub async fn ensure_group(&mut self, external_id: &SharedResourceIdentifier, name: &str) -> Result<(), KidsError> {
        let group_id = self.ensure_group_exists(external_id, name).await?;
        self.ensure_collection_exists(external_id, name, &group_id).await
    }

    async fn ensure_group_exists(&mut self, external_id: &SharedResourceIdentifier, name: &str) -> Result<String, KidsError> {
        if let Some(group) = self.state.groups.get_mut(external_id) {
            if group.name != name {
                let member_ids: Vec<_> = group.member_ids.iter().cloned().collect();
                self.apis
                    .vault
                    .update_group(&group.id, name, group.access_all, &self.state.access.of_group(&group.id), &member_ids)
                    .await?;
                group.name = name.to_owned();
            }
            return Ok(group.id.clone());
        }

        let group_id = self.apis.vault.create_group(name, external_id).await?;
        self.state.groups.insert(
            external_id.clone(),
            state::Group {
                id: group_id.clone(),
                name: name.to_owned(),
                access_all: false,
                member_ids: HashSet::new(),
            },
        );
        Ok(group_id)
    }

    async fn ensure_collection_exists(&mut self, external_id: &SharedResourceIdentifier, name: &str, group_id: &str) -> Result<(), KidsError> {
        let access = Access {
            id: group_id.to_owned(),
            flags: AccessFlags::EDIT,
        };

        if let Some(collection) = self.state.collections.get_mut(external_id) {
            let has_access = self.state.access.contains(group_id, &collection.id);
            if collection.name != name || !has_access {
                let mut groups = self.state.access.of_collection(&collection.id);
                if !has_access {
                    groups.push(access);
                }
                self.apis
                    .cli
                    .update_collection(&collection.id, name, external_id, &groups, &collection.members)
                    .await?;
                collection.name = name.to_owned();
                self.state.access.grant(group_id, &collection.id);
            }
            return Ok(());
        }

        let collection_id = self.apis.cli.create_collection(name, external_id, &[access]).await?;
        self.state.access.grant(group_id, &collection_id);
        self.state.collections.insert(
            external_id.clone(),
            state::Collection {
                id: collection_id,
                name: name.to_owned(),
                members: Vec::new(),
            },
        );
        Ok(())
    }

    /// Deletes the group and its collection. Vaultwarden keeps items from the collection in the organization,
    /// they only become unassigned. Owners and admins can still access them.
    pub async fn delete_group(&mut self, external_id: &SharedResourceIdentifier) -> Result<(), KidsError> {
        let Some(group) = self.state.groups.get(external_id) else {
            tracing::warn!(external_group_id = external_id, "Group not found, nothing to delete");
            return Ok(());
        };

        if let Some(collection) = self.state.collections.get(external_id) {
            self.apis.vault.delete_collection(&collection.id).await?;
            self.state.access.remove_collection(&collection.id);
            self.state.collections.remove(external_id);
        } else {
            tracing::warn!(external_group_id = external_id, "Collection of group not found, deleting only group");
        }

        self.apis.vault.delete_group(&group.id).await?;
        self.state.access.remove_group(&group.id);
        self.state.groups.remove(external_id);
        Ok(())
    }

    /// Ensures that the user is an enabled and confirmed member of the organization, belonging to exactly those
    /// managed groups whose external IDs are in `group_ids`.
    pub async fn ensure_user_active(
        &mut self,
        external_id: &SharedResourceIdentifier,
        email: Option<&str>,
        group_ids: &HashSet<SharedResourceIdentifier>,
    ) -> Result<(), KidsError> {
        if email.is_some_and(|email| self.is_ignored(email)) {
            tracing::debug!(source_user_id = external_id, "Skipping user with ignored email");
            return Ok(());
        }

        let needs_import = self
            .state
            .members
            .get(external_id)
            .is_none_or(|member| member.status == MembershipStatus::Revoked);
        if needs_import && !self.import_member(external_id, email).await? {
            return Ok(());
        }
        let member = self.state.members.get_mut(external_id).expect("the member exists or was just imported");

        if let Some(user) = self.state.users.get_mut(&member.user_id)
            && !user.enabled
        {
            self.apis.admin.enable_user(&member.user_id).await?;
            user.enabled = true;
        }

        if member.status == MembershipStatus::Accepted {
            self.apis.cli.confirm_member(&member.id).await?;
            member.status = MembershipStatus::Confirmed;
        }

        let member_id = member.id.clone();
        for (group_external_id, group) in &mut self.state.groups {
            let desired = group_ids.contains(group_external_id);
            if group.member_ids.contains(&member_id) == desired {
                continue;
            }
            let mut member_ids = group.member_ids.clone();
            if desired {
                member_ids.insert(member_id.clone());
            } else {
                member_ids.remove(&member_id);
            }
            let member_id_list: Vec<_> = member_ids.iter().cloned().collect();
            self.apis
                .vault
                .update_group(
                    &group.id,
                    &group.name,
                    group.access_all,
                    &self.state.access.of_group(&group.id),
                    &member_id_list,
                )
                .await?;
            group.member_ids = member_ids;
        }

        for group_id in group_ids.iter().filter(|group_id| !self.state.groups.contains_key(*group_id)) {
            tracing::warn!(
                source_user_id = external_id,
                source_group_id = group_id,
                "Group does not exist in Vaultwarden, skipping membership"
            );
        }
        Ok(())
    }

    /// Invites the user or restores their revoked membership, then loads the membership into the state.
    /// Returns whether the membership could be loaded.
    async fn import_member(&mut self, external_id: &SharedResourceIdentifier, email: Option<&str>) -> Result<bool, KidsError> {
        // A revoked member is matched by its email in Vaultwarden, which may differ from the source's.
        let known_email = self.state.members.get(external_id).map(|member| member.email.clone());
        let Some(email) = known_email.as_deref().or(email) else {
            tracing::warn!(source_user_id = external_id, "User has no email address to invite them with, skipping");
            return Ok(false);
        };

        // Vaultwarden would move the membership over to this user, so a source user with a forged
        // email could take over another user's membership.
        let other_owner = self
            .state
            .members
            .iter()
            .find(|(other_external_id, member)| *other_external_id != external_id && member.email.eq_ignore_ascii_case(email));
        if let Some((other_external_id, _)) = other_owner {
            tracing::warn!(
                source_user_id = external_id,
                other_source_user_id = other_external_id,
                "Email belongs to the membership of another user, skipping"
            );
            return Ok(false);
        }

        self.apis.directory.import_member(email, external_id).await?;

        // The import does not return the membership, so it has to be looked up.
        let member = self
            .apis
            .vault
            .get_members()
            .await?
            .into_iter()
            .find(|member| member.external_id.as_ref() == Some(external_id));
        let Some(member) = member else {
            tracing::warn!(
                source_user_id = external_id,
                "Membership not found after importing the user, will retry on next sync"
            );
            return Ok(false);
        };
        let Some(user) = self.apis.admin.get_user(&member.user_id).await? else {
            tracing::warn!(
                source_user_id = external_id,
                "Account not found after importing the user, will retry on next sync"
            );
            return Ok(false);
        };

        self.state.users.insert(user.id.clone(), user.into());
        self.state.members.insert(external_id.clone(), member.into());
        Ok(true)
    }

    /// Ensures that the user's account is disabled and their membership is revoked.
    ///
    /// Revoked members keep their external ID, so that they are still recognized when they become active again.
    pub async fn ensure_user_inactive(&mut self, external_id: &SharedResourceIdentifier, email: Option<&str>) -> Result<(), KidsError> {
        let user_id = match self.state.members.get(external_id) {
            Some(member) => member.user_id.clone(),
            None => {
                // Without a membership, the user may still have an account on the server.
                let Some((user_id, _)) = email.and_then(|email| self.state.user_by_email(email)) else {
                    return Ok(());
                };
                user_id.clone()
            }
        };

        if let Some(user) = self.state.users.get_mut(&user_id)
            && user.enabled
        {
            self.apis.admin.disable_user(&user_id).await?;
            user.enabled = false;
        }

        if let Some(member) = self.state.members.get_mut(external_id)
            && member.status != MembershipStatus::Revoked
        {
            self.apis.directory.revoke_member(&member.email, external_id).await?;
            member.status = MembershipStatus::Revoked;
        }
        Ok(())
    }

    /// Deletes the user's account including their personal vault.
    pub async fn delete_user(&mut self, external_id: &SharedResourceIdentifier) -> Result<(), KidsError> {
        let Some(member) = self.state.members.get(external_id) else {
            tracing::warn!(source_user_id = external_id, "User not found, nothing to delete");
            return Ok(());
        };

        match self.apis.admin.delete_user(&member.user_id).await {
            Ok(()) => {}
            Err(error) if is_last_owner_error(&error) => {
                tracing::warn!(
                    source_user_id = external_id,
                    "Cannot delete the last owner of an organization, keeping the account"
                );
                return Ok(());
            }
            Err(error) => return Err(error),
        }

        // Deleting the account also deleted its membership and all access through it.
        let member = self.state.members.remove(external_id).expect("the member was looked up above");
        self.state.users.remove(&member.user_id);
        for group in self.state.groups.values_mut() {
            group.member_ids.remove(&member.id);
        }
        for collection in self.state.collections.values_mut() {
            collection.members.retain(|access| access.id != member.id);
        }
        Ok(())
    }
}

/// Vaultwarden refuses to delete the last confirmed owner of an organization. This should not fail the sync.
fn is_last_owner_error(error: &KidsError) -> bool {
    matches!(error, KidsError::ApiOperationFailed(_, 400, _, source) if source.to_string().contains("Can't delete last owner"))
}
