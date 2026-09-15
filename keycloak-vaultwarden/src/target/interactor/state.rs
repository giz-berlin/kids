use std::collections::{HashMap, HashSet};

use kids_lib::error::KidsError;
use kids_lib::types::SharedResourceIdentifier;

use super::Apis;
use crate::target::external::{admin, vault_api};
use crate::target::types::{Access, AccessFlags, MembershipStatus};

pub(super) struct User {
    pub(super) email: String,
    pub(super) enabled: bool,
}

impl From<admin::User> for User {
    fn from(user: admin::User) -> Self {
        Self {
            email: user.email,
            enabled: user.user_enabled,
        }
    }
}

pub(super) struct Member {
    pub(super) id: String,
    pub(super) user_id: String,
    pub(super) email: String,
    pub(super) status: MembershipStatus,
}

impl From<vault_api::Member> for Member {
    fn from(member: vault_api::Member) -> Self {
        Self {
            id: member.id,
            user_id: member.user_id,
            email: member.email,
            status: member.status,
        }
    }
}

pub(super) struct Group {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) access_all: bool,
    /// All members of the group, including those not managed by the syncer.
    pub(super) member_ids: HashSet<String>,
}

pub(super) struct Collection {
    pub(super) id: String,
    pub(super) name: String,
    /// Members with direct access, which the syncer does not manage but has to send along on updates.
    pub(super) members: Vec<Access>,
}

/// Which groups can access which collections, keyed by `(group ID, collection ID)`, including groups and
/// collections not managed by the syncer.
///
/// Vaultwarden stores each entry once, but lists it on both the group and the collection, and
/// updating either replaces its whole list.
#[derive(Default)]
pub(super) struct AccessMap(HashMap<(String, String), AccessFlags>);

impl AccessMap {
    pub(super) fn contains(&self, group_id: &str, collection_id: &str) -> bool {
        self.0.contains_key(&(group_id.to_owned(), collection_id.to_owned()))
    }

    /// Grants manage access. No-op if entry already exists.
    pub(super) fn grant(&mut self, group_id: &str, collection_id: &str) {
        self.0.entry((group_id.to_owned(), collection_id.to_owned())).or_insert(AccessFlags::EDIT);
    }

    pub(super) fn remove_group(&mut self, group_id: &str) {
        self.0.retain(|(id, _), _| id != group_id);
    }

    pub(super) fn remove_collection(&mut self, collection_id: &str) {
        self.0.retain(|(_, id), _| id != collection_id);
    }

    /// The collections the group can access.
    pub(super) fn of_group(&self, group_id: &str) -> Vec<Access> {
        self.0
            .iter()
            .filter(|((id, _), _)| id == group_id)
            .map(|((_, collection_id), flags)| Access {
                id: collection_id.clone(),
                flags: *flags,
            })
            .collect()
    }

    /// The groups that can access the collection.
    pub(super) fn of_collection(&self, collection_id: &str) -> Vec<Access> {
        self.0
            .iter()
            .filter(|((_, id), _)| id == collection_id)
            .map(|((group_id, _), flags)| Access {
                id: group_id.clone(),
                flags: *flags,
            })
            .collect()
    }
}

pub(super) struct TargetState {
    pub(super) users: HashMap<String, User>,
    pub(super) members: HashMap<SharedResourceIdentifier, Member>,
    pub(super) groups: HashMap<SharedResourceIdentifier, Group>,
    pub(super) collections: HashMap<SharedResourceIdentifier, Collection>,
    pub(super) access: AccessMap,
}

impl TargetState {
    pub(super) async fn fetch(apis: &Apis, ignored_emails: &HashSet<String>) -> Result<Self, KidsError> {
        let mut users = HashMap::new();
        for user in apis.admin.get_users().await? {
            if !ignored_emails.contains(&user.email.to_lowercase()) {
                users.insert(user.id.clone(), User::from(user));
            }
        }

        let mut members = HashMap::new();
        for member in apis.vault.get_members().await? {
            if let Some(external_id) = member.external_id.clone()
                && users.contains_key(&member.user_id)
            {
                members.insert(external_id, Member::from(member));
            }
        }

        let mut access = AccessMap::default();
        let mut groups = HashMap::new();
        for group in apis.vault.get_groups().await? {
            for collection in group.collections {
                access.0.insert((group.id.clone(), collection.id), collection.flags);
            }
            let Some(external_id) = group.external_id else {
                continue;
            };
            let member_ids = apis.vault.get_group_member_ids(&group.id).await?.into_iter().collect();
            groups.insert(
                external_id,
                Group {
                    id: group.id,
                    name: group.name,
                    access_all: group.access_all,
                    member_ids,
                },
            );
        }

        let mut collections = HashMap::new();
        for collection in apis.cli.get_collections().await? {
            let Some(external_id) = collection.external_id else {
                continue;
            };
            let members = apis.vault.get_collection_member_access(&collection.id).await?;
            collections.insert(
                external_id,
                Collection {
                    id: collection.id,
                    name: collection.name,
                    members,
                },
            );
        }

        Ok(Self {
            users,
            members,
            groups,
            collections,
            access,
        })
    }

    pub(super) fn user_by_email(&self, email: &str) -> Option<(&String, &User)> {
        self.users.iter().find(|(_, user)| user.email.eq_ignore_ascii_case(email))
    }
}
