use crate::{error, types};
use std::{collections, fmt, rc};

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
#[async_trait::async_trait(?Send)]
pub trait Source {
    /// The configuration struct to use for a specific [Source].
    /// Must derive from [serde::de::DeserializeOwned] because it will be deserialized from a
    /// TOML configuration file.
    type Config: serde::de::DeserializeOwned;

    fn info(&self) -> String;

    /// Construct a new [Source] using the source-specific [Self::Config].
    fn new(config: Self::Config) -> Self;

    /// All [Group]s present within the [Source] (in a specific context, for example all groups visible to a Keycloak client within a Keycloak realm).
    async fn all_groups(&self) -> Result<Vec<rc::Rc<dyn Group>>, error::KidsError>;
    /// All [User]s present within the [Source] (in a specific context, for example all groups visible to a Keycloak client within a Keycloak realm).
    async fn all_users(&self) -> Result<Vec<Box<dyn User>>, error::KidsError>;
}

/// A user entity within a data [Source].
#[async_trait::async_trait(?Send)]
pub trait User {
    /// Identifier of the [User].
    fn id(&self) -> &types::SharedResourceIdentifier;
    /// Whether this [User] is active: Users may still be present within the source even if they
    /// are no longer allowed to log in.
    fn enabled(&self) -> bool;
    fn username(&self) -> Option<&str>;
    fn email(&self) -> Option<&str>;
    /// A map containing all additional user attributes.
    /// Many [Targets](crate::target::interface::Target) will make use of custom user attributes to store target-system-specific
    /// configuration for the user.
    fn attributes(&self) -> &collections::HashMap<String, Vec<String>>;
    fn roles(&self) -> &Vec<String>; // client_roles, realm_role;

    /// All [Group]s the [User] is in.
    async fn groups(&self) -> Result<Vec<rc::Rc<dyn Group>>, error::KidsError>;
}

impl fmt::Debug for dyn User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("dyn User")
            .field("id", &self.id())
            .field("enabled", &self.enabled())
            .field("username", &self.username())
            .field("email", &self.email())
            .field("attributes", &self.attributes())
            .field("roles", &self.roles())
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

    // Note: A reference to the users of a group is only needed if a target::Target wants to propagate
    // group attributes to users in some way.
    // There is no such target at the moment, but there might be in the future.
    // fn users(&self) -> Vec<Box<dyn User>>;

    /// Farthest ancestor of this [Group]. If this group itself is a root group, returns this group.
    fn root_group(self: rc::Rc<Self>) -> rc::Rc<dyn Group>;
    /// The direct parent of this [Group]. Will return [None] if this group is a root group.
    fn parent_group(&self) -> Option<rc::Rc<dyn Group>>;
    /// All direct subgroups of this [Group]. Will not contain transitive subgroups (i.e. grandchildren or deeper).
    async fn sub_groups(self: rc::Rc<Self>) -> Result<Vec<rc::Rc<dyn Group>>, error::KidsError>;
}

impl fmt::Debug for dyn Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("dyn Group")
            .field("id", &self.id())
            .field("name", &self.name())
            .field("path", &self.path())
            .finish()
    }
}
