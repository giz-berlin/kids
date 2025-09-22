use crate::target::interface;
use crate::{error, source, types};
use std::{collections, rc};

#[derive(serde::Deserialize)]
pub struct JamesConfig {}

pub struct Connector {}

#[async_trait::async_trait(?Send)]
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

    async fn all_groups(&self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        todo!()
    }

    async fn all_users(&self) -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        todo!()
    }

    async fn delete_group(&mut self, _group_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn delete_user(&mut self, _user_id: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn create_or_update_group(&mut self, _group: rc::Rc<dyn source::interface::Group>) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn create_or_update_user(&mut self, _user: Box<dyn source::interface::User>) -> Result<(), error::KidsError> {
        todo!()
    }
}
