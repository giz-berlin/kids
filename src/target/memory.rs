use crate::target::interface;
use crate::{error, source, types};
use std::collections::{self, HashMap, HashSet};
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub struct InMemoryConfig {}

/// The memory connector provides a target implementation which stores users and groups entirely in-memory.
/// This enables easy testing of core functionality without having to spin up an external service.
pub struct Connector {
    users: HashMap<types::SharedResourceIdentifier, Arc<dyn source::interface::User + Sync + Send>>,
    groups: HashMap<types::SharedResourceIdentifier, Arc<dyn source::interface::Group + Sync + Send>>,
}

#[async_trait::async_trait]
impl interface::Target for Connector {
    type Config = InMemoryConfig;

    async fn new(_config: Self::Config) -> Result<Self, error::KidsError> {
        let connector = Connector {
            users: HashMap::new(),
            groups: HashMap::new(),
        };

        Ok(connector)
    }

    fn info(&self) -> String {
        "In-Memory Connector".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), error::KidsError> {
        Ok(())
    }

    async fn all_groups(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.groups.keys().cloned().collect())
    }

    async fn all_users(&mut self) -> Result<HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        Ok(self.users.keys().cloned().collect())
    }

    async fn delete_group(&mut self, group_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        self.groups.remove(&group_id);
        Ok(())
    }

    async fn delete_user(&mut self, user_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        self.users.remove(&user_id);
        Ok(())
    }

    async fn create_or_update_group(&mut self, group: Arc<dyn source::interface::Group + Sync + Send>) -> Result<(), error::KidsError> {
        self.groups.insert(group.id().to_owned(), group);
        Ok(())
    }

    async fn create_or_update_user(&mut self, user: Arc<dyn source::interface::User + Sync + Send>) -> Result<(), error::KidsError> {
        self.users.insert(user.id().to_owned(), user);
        Ok(())
    }
}
