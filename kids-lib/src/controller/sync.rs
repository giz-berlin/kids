use anyhow::Context;

use crate::types;

/// Performs a full synchronization from the source (e.g. Keycloak) to the target (e.g. Synapse) to get the target into a known
/// good state. This ensure a correct state when starting the syncer but also ensures that lost updates are applied eventually.
///
/// Since this function mutates the target, exclusive access to it must be guaranteed by the caller e.g. by using an RWLock on the
/// target instance and further incremental updates should be queued until this function completes. Otherwise, newer updates may be
/// overwritten by older state from this sync.
///
/// The sync synchronizes groups first and users second to ensure that groups are present before users are added to them.
/// The deletion order within a group hierarchy is not guaranteed and targets must handle cascading deletes themselves.
pub async fn full_sync<S: crate::interface::source::Source + Send + Sync + 'static, T: crate::interface::target::Target + Send + Sync + 'static>(
    source: &S,
    target: &mut T,
) -> anyhow::Result<()> {
    tracing::info!("Starting full sync");

    target.full_sync_incoming().await.context("preparing target for full sync")?;

    // 1. Synchronize groups first to ensure that new groups are created before user memberships for these new groups are updated.
    let source_groups = source.all_groups().await.context("querying all groups from source")?;
    let target_groups = target.all_groups().await.context("querying all groups from target")?;

    tracing::info!(source = source_groups.len(), target = target_groups.len(), "Synchronizing groups");

    let mut source_group_set: std::collections::HashSet<types::SharedResourceIdentifier> = std::collections::HashSet::new();
    for group in source_groups {
        tracing::debug!(id = group.id(), "Upserting group");

        source_group_set.insert(group.id().to_owned());
        target.create_or_update_group(group).await.context("upserting new group in target")?;
    }

    for group in target_groups {
        if !source_group_set.contains(&group) {
            tracing::warn!(id = group, "Deleting leftover group from source");
            // Deletion order within a group hierarchy is not guaranteed: once a group is gone
            // from the source its subgroup relationships are unknown. Targets that track group
            // hierarchies must handle cascading deletes themselves and any remaining subgroups will
            // be cleaned up on the next full sync.
            target.delete_group(&group).await.context("deleting leftover group from source")?;
        }
    }

    // 2. Synchronize users
    let source_users = source.all_users().await.context("querying all users from source")?;
    let target_users = target.all_users().await.context("querying all users from target")?;

    tracing::info!(source = source_users.len(), target = target_users.len(), "Synchronizing users");

    let mut source_user_set: std::collections::HashSet<types::SharedResourceIdentifier> = std::collections::HashSet::new();
    for user in source_users {
        tracing::debug!(id = user.id(), "Upserting user");

        source_user_set.insert(user.id().to_owned());
        target.create_or_update_user(user).await.context("upserting new user in target")?;
    }

    for user in target_users {
        if !source_user_set.contains(&user) {
            tracing::warn!(id = user, "Deleting leftover user from source");
            target.delete_user(&user).await.context("deleting leftover user from source")?;
        }
    }

    Ok(())
}
