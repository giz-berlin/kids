use std::collections::{HashMap, HashSet};

use kids_lib::error::KidsError;
use kids_lib::types::SharedResourceIdentifier;

use super::Apis;
use crate::target::external::{admin, vault_api};
use crate::target::types::{
    AccessFlags, CollectionAccess, CollectionId, Email, GroupAccess, GroupId, MemberAccess, MembershipId, MembershipStatus, MembershipType, UserId,
};

/// A user's account.
#[derive(Debug)]
pub(super) struct User {
    pub(super) email: Email,
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

/// A user's membership in an organization.
#[derive(Debug)]
pub(super) struct Membership {
    /// ID of the membership relation.
    pub(super) id: MembershipId,
    /// ID of the user account.
    pub(super) user_id: UserId,
    /// Email of the user's account.
    pub(super) email: Email,
    /// Status of the membership relation.
    pub(super) status: MembershipStatus,

    pub(super) membership_type: MembershipType,
}

impl From<vault_api::Membership> for Membership {
    fn from(membership: vault_api::Membership) -> Self {
        Self {
            id: membership.id,
            user_id: membership.user_id,
            email: membership.email,
            status: membership.status,
            membership_type: membership.membership_type,
        }
    }
}

/// A group on an organization.
/// Used to manage grouped access to collections.
#[derive(Debug)]
pub(super) struct Group {
    /// ID of the group.
    pub(super) id: GroupId,
    /// Name of the group.
    pub(super) name: String,
    /// Whether members of the group can access *all* collections in an organization,
    /// regardless of collection access that is explicitly configured.
    pub(super) access_all: bool,
    /// All IDs of memberships in the group, including those not managed by the syncer.
    pub(super) member_ids: HashSet<MembershipId>,
}

/// A collection holding vault items.
#[derive(Debug)]
pub(super) struct Collection {
    /// ID of the collection.
    pub(super) id: CollectionId,
    /// Name of the collection.
    pub(super) name: String,
    /// Members with direct access.
    /// Not managed by the syncer, but have to be sent along on updates.
    pub(super) members: Vec<MemberAccess>,
}

/// Which groups can access which collections, keyed by `(group ID, collection ID)`, including groups and
/// collections not managed by the syncer.
///
/// Vaultwarden stores each entry once, but lists it on both the group and the collection, and
/// updating either replaces its whole list.
#[derive(Default)]
pub(super) struct AccessMap(HashMap<(GroupId, CollectionId), AccessFlags>);

impl AccessMap {
    pub(super) fn contains(&self, group_id: &GroupId, collection_id: &CollectionId) -> bool {
        self.0.contains_key(&(group_id.to_owned(), collection_id.to_owned()))
    }

    /// Grants edit access. No-op if entry already exists.
    pub(super) fn grant(&mut self, group_id: &GroupId, collection_id: &CollectionId) {
        self.0.entry((group_id.to_owned(), collection_id.to_owned())).or_insert(AccessFlags::EDIT);
    }

    pub(super) fn remove_group(&mut self, group_id: &GroupId) {
        self.0.retain(|(id, _), _| id != group_id);
    }

    pub(super) fn remove_collection(&mut self, collection_id: &CollectionId) {
        self.0.retain(|(_, id), _| id != collection_id);
    }

    /// The collections the group can access.
    pub(super) fn of_group(&self, group_id: &GroupId) -> Vec<CollectionAccess> {
        self.0
            .iter()
            .filter(|((id, _), _)| id == group_id)
            .map(|((_, collection_id), flags)| CollectionAccess {
                id: collection_id.clone(),
                flags: *flags,
            })
            .collect()
    }

    /// The groups that can access the collection.
    pub(super) fn of_collection(&self, collection_id: &CollectionId) -> Vec<GroupAccess> {
        self.0
            .iter()
            .filter(|((_, id), _)| id == collection_id)
            .map(|((group_id, _), flags)| GroupAccess {
                id: group_id.clone(),
                flags: *flags,
            })
            .collect()
    }
}

/// Current (cached) state of the Vaultwarden.
///
/// This is, for example, required to send along full access maps on every
/// update, since they always fully overwrite the entries in the database.
pub(super) struct VaultwardenState {
    pub(super) users: HashMap<UserId, User>,
    pub(super) members: HashMap<SharedResourceIdentifier, Membership>,
    pub(super) groups: HashMap<SharedResourceIdentifier, Group>,
    pub(super) collections: HashMap<SharedResourceIdentifier, Collection>,
    pub(super) access: AccessMap,
}

impl VaultwardenState {
    pub(super) async fn fetch(apis: &Apis, ignored_emails: &HashSet<Email>) -> Result<Self, KidsError> {
        let mut users: HashMap<UserId, User> = HashMap::new();
        for user in apis.admin.get_users().await? {
            if !ignored_emails.contains(&user.email) {
                users.insert(user.id.clone(), User::from(user));
            }
        }

        let mut members = HashMap::new();
        for member in apis.vault.get_members().await? {
            if let Some(external_id) = member.external_id.clone() {
                members.insert(external_id, Membership::from(member));
            }
        }

        let mut access = AccessMap::default();
        let mut groups = HashMap::new();
        for group in apis.vault.get_groups().await? {
            let Some(external_id) = group.external_id else {
                continue;
            };
            for collection in group.collections {
                access.0.insert((group.id.clone(), collection.id), collection.flags);
            }
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

    pub(super) fn user_by_email(&self, email: &Email) -> Option<(&UserId, &User)> {
        self.users.iter().find(|(_, user)| user.email == *email)
    }
}
