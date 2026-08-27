use std::collections::{self, HashMap, HashSet};
use std::sync::Arc;

use kids_lib::error::KidsError;

#[derive(serde::Deserialize)]
pub struct InMemoryConfig {}

/// The memory connector provides a target implementation which stores users and groups entirely in-memory.
/// This enables easy testing of core functionality without having to spin up an external service.
pub struct Connector {
    users: HashMap<kids_lib::types::SharedResourceIdentifier, Arc<dyn kids_lib::interface::source::User + Sync + Send>>,
    groups: HashMap<kids_lib::types::SharedResourceIdentifier, Arc<dyn kids_lib::interface::source::Group + Sync + Send>>,
}

#[async_trait::async_trait]
impl kids_lib::interface::target::Target for Connector {
    type Config = InMemoryConfig;

    async fn new(_config: Self::Config) -> Result<Self, KidsError> {
        let connector = Connector {
            users: HashMap::new(),
            groups: HashMap::new(),
        };

        Ok(connector)
    }

    fn info(&self) -> String {
        "In-Memory Connector".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), KidsError> {
        Ok(())
    }

    async fn all_groups(&mut self) -> Result<collections::HashSet<kids_lib::types::SharedResourceIdentifier>, KidsError> {
        Ok(self.groups.keys().cloned().collect())
    }

    async fn all_users(&mut self) -> Result<HashSet<kids_lib::types::SharedResourceIdentifier>, KidsError> {
        Ok(self.users.keys().cloned().collect())
    }

    async fn delete_group(&mut self, group_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), KidsError> {
        self.groups.remove(group_id);
        Ok(())
    }

    async fn delete_user(&mut self, user_id: &kids_lib::types::SharedResourceIdentifier) -> Result<(), KidsError> {
        self.users.remove(user_id);
        Ok(())
    }

    async fn create_or_update_group(&mut self, group: Arc<dyn kids_lib::interface::source::Group + Sync + Send>) -> Result<(), KidsError> {
        self.groups.insert(group.id().to_owned(), group);
        Ok(())
    }

    async fn create_or_update_user(&mut self, user: Arc<dyn kids_lib::interface::source::User + Sync + Send>) -> Result<(), KidsError> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            match user.groups(false).await {
                Ok(groups) => tracing::debug!(user_id = user.id(), groups = ?group_paths(&groups), "Resolved direct groups for user"),
                Err(err) => tracing::warn!(user_id = user.id(), error = %err, "Failed to resolve direct groups for user"),
            }
            match user.groups(true).await {
                Ok(groups) => tracing::debug!(user_id = user.id(), groups = ?group_paths(&groups), "Resolved transitive groups for user"),
                Err(err) => tracing::warn!(user_id = user.id(), error = %err, "Failed to resolve transitive groups for user"),
            }
        }

        self.users.insert(user.id().to_owned(), user);
        Ok(())
    }
}

fn group_paths(groups: &[Arc<dyn kids_lib::interface::source::Group + Sync + Send>]) -> Vec<&str> {
    groups.iter().map(|g| g.path()).collect()
}
