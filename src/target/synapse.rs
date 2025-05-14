use crate::target::interface;
use crate::{error, source, types};
use std::{collections, rc};

#[derive(serde::Deserialize)]
pub struct SynapseConfig {
    pub hello: String,
}

pub struct Connector {}

#[async_trait::async_trait(?Send)]
impl interface::Target for Connector {
    type Config = SynapseConfig;

    fn new(_: Self::Config) -> Self {
        Connector {}
    }

    fn info(&self) -> String {
        "Synapse Connector!".to_string()
    }

    async fn all_groups() -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        todo!()
    }

    async fn all_users() -> Result<collections::HashSet<types::SharedResourceIdentifier>, error::KidsError> {
        todo!()
    }

    async fn delete_group(_group: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn delete_user(_user: types::SharedResourceIdentifier) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn create_or_update_group(_group: rc::Rc<dyn source::interface::Group>) -> Result<(), error::KidsError> {
        todo!()
    }

    async fn create_or_update_user(_user: Box<dyn source::interface::User>) -> Result<(), error::KidsError> {
        todo!()
    }
}
