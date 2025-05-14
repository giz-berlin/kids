use crate::{error, source, types};
use std::{collections, rc};

/// A target system to be kept up-to-date by the syncer.
/// Responsible for determining which users and groups are currently present in the target system
/// and to perform CRUD operations on those as instructed by the [crate::controller].
#[async_trait::async_trait(?Send)]
pub trait Target {
    /// The configuration struct to use for a specific [Target].
    /// Must derive from [serde::de::DeserializeOwned] because it will be deserialized from a
    /// TOML configuration file.
    type Config: serde::de::DeserializeOwned;

    /// Construct a new [Target] using the target-specific [Self::Config].
    fn new(config: Self::Config) -> Self;

    fn info(&self) -> String;

    /// Set of groups present in the target system.
    /// Only returns [identifiers](types::SharedResourceIdentifier) because the ground truth for the
    /// group attributes is the [Source](source::interface::Source).
    async fn all_groups() -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError>;
    /// Set of users present in the target system.
    /// Only returns [identifiers](types::SharedResourceIdentifier) because the ground truth for the
    /// user attributes is the [Source](source::interface::Source).
    async fn all_users() -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError>;

    /// Delete the group with the given identifier from the target system.
    async fn delete_group(group: types::SharedResourceIdentifier) -> Result<(), error::KidsError>;
    /// Delete the user with the given identifier from the target system.
    async fn delete_user(user: types::SharedResourceIdentifier) -> Result<(), error::KidsError>;

    /// Update group attributes in the target system to match those of the given [source group](source::interface::Group).
    /// If the group is not yet present in the target system, create it.
    /// Will *not* manage group membership of users.
    ///
    /// When dealing with a group hierarchy, this method should be called for the parent groups
    /// before the child groups.
    async fn create_or_update_group(group: rc::Rc<dyn source::interface::Group>) -> Result<(), error::KidsError>;

    /// Update user attributes in the target system to match those of the given [source user](source::interface::User).
    /// If the user is not yet present in the target system, create it.
    ///
    /// Will manage group memberships of the user. Therefore, must be called **after** [Self::create_or_update_group]
    /// for the referenced groups.
    async fn create_or_update_user(user: Box<dyn source::interface::User>) -> Result<(), error::KidsError>;
}
