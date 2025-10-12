use crate::target::interface;
use crate::{error, source, types};
use std::collections;

#[derive(serde::Deserialize)]
pub struct JamesConfig {}

pub struct Connector {}

#[async_trait::async_trait]
impl interface::Target for Connector {
    type Config = JamesConfig;

    async fn new(_: Self::Config) -> Result<Self, error::KidsError> {
        Ok(Connector {})
    }

    fn info(&self) -> String {
        "James Connector!".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn all_groups(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        todo!()
    }

    async fn all_users(&mut self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        todo!()
    }

    async fn delete_group(&mut self, _group_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn delete_user(&mut self, _user_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn create_or_update_group(&mut self, _group: std::sync::Arc<Box<dyn source::interface::Group + Sync + Send>>) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn create_or_update_user(&mut self, _user: std::sync::Arc<Box<dyn source::interface::User + Sync + Send>>) -> Result<(), error::KidsError> {
        todo!()
    }
}
