use crate::{error, source, types};
use std::collections;

/// A target system to be kept up-to-date by the syncer.
/// Responsible for determining which users and groups are currently present in the target system
/// and to perform CRUD operations on those as instructed by the [crate::controller].
#[async_trait::async_trait]
pub trait Target: Sized {
    /// The configuration struct to use for a specific [Target].
    /// Must derive from [serde::de::DeserializeOwned] because it will be deserialized from a
    /// TOML configuration file.
    type Config: serde::de::DeserializeOwned;

    /// Construct a new [Target] using the target-specific [Self::Config].
    async fn new(config: Self::Config) -> Result<Self, error::KidsError>;

    fn info(&self) -> String;

    /// Indicates to the target that the controller is about to start a full sync.
    /// This allows the target to perform some preparatory actions, such as constructing necessary
    /// internal state.
    /// If the target is caching any information, these caches should be invalidated and/or
    /// rebuilt now.
    async fn full_sync_incoming(&mut self) -> Result<(), error::KidsError>;

    /// Set of groups present in the target system.
    /// Only returns [identifiers](types::SharedResourceIdentifier) because the ground truth for the
    /// group attributes is the [Source](source::interface::Source).
    async fn all_groups(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError>;
    /// Set of users present in the target system.
    /// Only returns [identifiers](types::SharedResourceIdentifier) because the ground truth for the
    /// user attributes is the [Source](source::interface::Source).
    async fn all_users(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError>;

    /// Delete the group with the given identifier from the target system.
    /// When the group doesn't exist the operation should be considered successful and no error should be returned.
    async fn delete_group(&mut self, group_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError>;
    /// Delete the user with the given identifier from the target system.
    /// When the user doesn't exist the operation should be considered successful and no error should be returned.
    async fn delete_user(&mut self, user_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError>;

    /// Update group attributes in the target system to match those of the given [source group](source::interface::Group).
    /// If the group is not yet present in the target system, create it.
    /// Will *not* manage group membership of users.
    ///
    /// When dealing with a group hierarchy, this method should be called for the parent groups
    /// before the child groups.
    async fn create_or_update_group(&mut self, group: std::sync::Arc<dyn source::interface::Group + Send + Sync>) -> Result<(), error::KidsError>;

    /// Update user attributes in the target system to match those of the given [source user](source::interface::User).
    /// If the user is not yet present in the target system, create it.
    ///
    /// Will manage group memberships of the user. Therefore, must be called **after** [Self::create_or_update_group]
    /// for the referenced groups.
    async fn create_or_update_user(&mut self, user: std::sync::Arc<dyn source::interface::User + Send + Sync>) -> Result<(), error::KidsError>;
}
