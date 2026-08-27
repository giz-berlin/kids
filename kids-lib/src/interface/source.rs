use crate::{error, types};
use std::{collections, fmt};
// WHY WE ARE NOT USING ITERATORS IN THESE INTERFACES:
// Currently, the traits specified in this file demand that all data is fetched from the source
// at once (i.e., the methods return vectors of groups and users) instead of allowing for an
// iterator-based approach where data is fetched on demand.
// This is because our primary data source (Keycloak) does not implement Paginators.
// Keycloak does allow fetching users and groups in pages, but the pagination is not pinned to a
// point in time. If entities are created or deleted while we iterate over the pages, it affects
// the data we receive.
// This can cause two issues:
// 1. user A created in one of the pages <=N we have already read
// -> exactly one user B is shifted from page N to page N+1
// -> B was already received with page N, we now read B a second time with page N+1
// 2. user A deleted in one of the pages <=N we have already read
// -> exactly one user B is shifted from page N+1 to page N
// -> we did not read B with page N, it is also no longer contained in page N+1
// -> we miss user B
// We are able to handle issue 1, if necessary, but issue 2 is not acceptable, as the syncer would
// proceed to delete the user B (that we missed) in the target, even though it still exists.
// Currently, the only option to avoid this issue is to have Keycloak return all desired entities
// in a single request, as even directly successive requests could run into race conditions
// with external operations performed on the Keycloak.
//
// Note that we COULD still have the traits return iterators, but these would need to be async (as
// fetching data on demand is an asynchronous operation) and async iterators are not well-supported
// in Rust at the moment, so - at least for now - we simplify working with the traits by having them
// return vectors instead.

/// A data source of the syncer. In concrete instantiations, it will typically be a connection to an external data store.
/// [User] and [Group] information obtained from the source is considered to be the ground truth; the main purpose of this software is to
/// synchronize the data to a [Target](crate::target::interface::Target).
/// Note that this trait only provides methods for obtaining full lists of [User]s and [Group]s present in the source, as the data
/// entities directly provide methods for accessing related ones (for example, [Group::sub_groups]).
#[async_trait::async_trait]
pub trait Source {
    /// The configuration struct to use for a specific [Source].
    /// Must derive from [serde::de::DeserializeOwned] because it will be deserialized from a
    /// TOML configuration file.
    type Config: serde::de::DeserializeOwned;
    type UserWebhookPayload: serde::de::DeserializeOwned + Send + Sync + schemars::JsonSchema;
    type GroupWebhookPayload: serde::de::DeserializeOwned + Send + Sync + schemars::JsonSchema;

    fn info(&self) -> String;

    /// Construct a new [Source] using the source-specific [Self::Config].
    fn new(config: Self::Config) -> Self;

    /// All [Group]s present within the [Source] (in a specific context, for example all groups visible to a Keycloak client within a Keycloak realm).
    async fn all_groups(&self) -> Result<Vec<std::sync::Arc<dyn Group + Send + Sync>>, error::KidsError>;
    /// All [User]s present within the [Source] (in a specific context, for example all groups visible to a Keycloak client within a Keycloak realm).
    async fn all_users(&self) -> Result<Vec<std::sync::Arc<dyn User + Send + Sync>>, error::KidsError>;

    async fn user_from_webhook(&self, payload: Self::UserWebhookPayload) -> Result<Box<dyn User + Send + Sync>, error::KidsError>;
    fn group_from_webhook(&self, payload: Self::GroupWebhookPayload) -> Box<dyn Group + Send + Sync>;
}

/// A user entity within a data [Source].
#[async_trait::async_trait]
pub trait User {
    /// Identifier of the [User].
    fn id(&self) -> &types::SharedResourceIdentifier;
    /// Whether this [User] is active: Users may still be present within the source even if they
    /// are no longer allowed to log in.
    fn enabled(&self) -> bool;
    fn username(&self) -> Option<&str>;
    fn first_name(&self) -> Option<&str>;
    fn last_name(&self) -> Option<&str>;
    /// Get the name of the user that is human-friendly and can be used to display their name.
    fn display_name(&self) -> Option<String> {
        match (self.first_name(), self.last_name()) {
            (Some(first_name), Some(last_name)) => Some(format!("{first_name} {last_name}")),
            (Some(first_name), None) => Some(first_name.to_owned()),
            (None, Some(last_name)) => Some(last_name.to_owned()),
            (None, None) => None,
        }
    }
    fn email(&self) -> Option<&str>;
    /// A map containing all additional user attributes.
    /// Many [Targets](crate::target::interface::Target) will make use of custom user attributes to store target-system-specific
    /// configuration for the user.
    fn attributes(&self) -> &collections::HashMap<String, Vec<String>>;

    /// All [Group]s the [User] is in.
    /// If `include_transitive_groups` is `true`, the result also contains every (indirect) parent of each group the [User] is
    /// directly in. For example, if the [User] is a direct member of a group with path `/Group1/Group2/Group3`,
    /// the result will contain `Group1`, `Group2` and `Group3`, instead of just `Group3`.
    async fn groups(&self, include_transitive_groups: bool) -> Result<Vec<std::sync::Arc<dyn Group + Send + Sync>>, error::KidsError>;

    /// All roles the [User] has attached.
    async fn roles(&self) -> Result<Vec<String>, error::KidsError>;
}

impl fmt::Debug for dyn User + Send + Sync {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("dyn User")
            .field("id", &self.id())
            .field("enabled", &self.enabled())
            .field("username", &self.username())
            .field("email", &self.email())
            .field("attributes", &self.attributes())
            .finish()
    }
}

/// A group entity within a data [Source].
#[async_trait::async_trait(?Send)]
pub trait Group {
    /// Identifier of the [Group].
    fn id(&self) -> &types::SharedResourceIdentifier;
    fn name(&self) -> &str;
    /// *Display* path of the [Group]. Must **not** be used as an identifier, as it might be ambiguous.
    /// For example, both a group named "A/B" and a subgroup B of group A might receive the same path "/A/B".
    fn path(&self) -> &str;

    /// A map containing all additional group attributes.
    /// Many [Targets](crate::target::interface::Target) will make use of custom group attributes to
    /// retrieve target-system-specific configuration for the group.
    fn attributes(&self) -> &collections::HashMap<String, Vec<String>>;

    // Note: A reference to the users of a group is only needed if a target::Target wants to propagate
    // group attributes to users in some way.
    // There is no such target at the moment, but there might be in the future.
    // fn users(&self) -> Vec<Box<dyn User>>;

    /// Farthest ancestor of this [Group]. If this group itself is a root group, returns this group.
    fn root_group(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn Group>;
    /// The direct parent of this [Group]. Will return [None] if this group is a root group.
    fn parent_group(&self) -> Option<std::sync::Arc<dyn Group>>;
    /// All direct subgroups of this [Group]. Will not contain transitive subgroups (i.e. grandchildren or deeper).
    async fn sub_groups(self: std::sync::Arc<Self>) -> Result<Vec<std::sync::Arc<dyn Group>>, error::KidsError>;
}

impl fmt::Debug for dyn Group + Send + Sync {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("dyn Group")
            .field("id", &self.id())
            .field("name", &self.name())
            .field("path", &self.path())
            .finish()
    }
}
