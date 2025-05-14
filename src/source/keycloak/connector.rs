use crate::source::interface;
use crate::{config, error};
use std::rc;

/// A connector to Keycloak providing the [Source](interface::Source) interface.
pub struct Connector {}

#[async_trait::async_trait(?Send)]
impl interface::Source for Connector {
    type Config = config::EmptyConfig;

    fn info(&self) -> String {
        "Keycloak Connector!".to_string()
    }

    fn new(_config: Self::Config) -> Self {
        Connector {}
    }

    async fn all_groups(&self) -> Result<Vec<rc::Rc<dyn interface::Group>>, error::KidsError> {
        todo!()
    }

    async fn all_users(&self) -> Result<Vec<Box<dyn interface::User>>, error::KidsError> {
        todo!()
    }
}
