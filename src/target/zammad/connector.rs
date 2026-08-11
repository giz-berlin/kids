#[derive(serde::Deserialize)]
pub struct ZammadConfig {
    zammad_api: super::external::ZammadApiConfig,
}

pub struct Connector<ZammadApi: super::external::ZammadApi> {
    client: ZammadApi,
}

impl<ZammadApi: super::external::ZammadApi + std::marker::Send> Connector<ZammadApi> {
    fn info(&self) -> String {
        "Zammad Connector!".to_string()
    }

    /// See [`full_sync_incoming` of `crate::target::interface::Target`](crate::target::interface::Target::full_sync_incoming) for reference.
    async fn full_sync_incoming(&mut self) -> Result<(), crate::error::KidsError> {
        todo!()
    }

    /// Return the identifiers of all [Source Groups](crate::source::interface::Group) known to Zammad.
    /// As we do not manage groups here, this will always return an empty collection.
    async fn all_groups(&mut self) -> Result<std::collections::HashSet<crate::types::SharedResourceIdentifier>, crate::error::KidsError> {
        Ok(std::collections::HashSet::new())
    }

    /// See [`all_users` of `crate::target::interface::Target`](crate::target::interface::Target::all_users) for reference.
    async fn all_users(&mut self) -> Result<std::collections::HashSet<crate::types::SharedResourceIdentifier>, crate::error::KidsError> {
        todo!()
    }

    /// Delete the entity in Zammad relating to the identifier `source_group_id`.
    /// As we do not manage groups here, this will always return success without doing anything collection.
    async fn delete_group(&mut self, _source_group_id: &crate::types::SharedResourceIdentifier) -> Result<(), crate::error::KidsError> {
        Ok(())
    }

    /// See [`delete_user` of `crate::target::interface::Target`](crate::target::interface::Target::delete_user) for reference.
    async fn delete_user(&mut self, user_id: &crate::types::SharedResourceIdentifier) -> Result<(), crate::error::KidsError> {
        todo!()
    }

    /// Create or update the entity in Zammad relating to the source group `source_group`.
    /// As we do not manage groups here, this will always return success without doing anything collection.
    async fn create_or_update_group(
        &mut self,
        _source_group: std::sync::Arc<dyn crate::source::interface::Group + Send + Sync>,
    ) -> Result<(), crate::error::KidsError> {
        Ok(())
    }

    /// See [`create_or_update_user` of `crate::target::interface::Target`](crate::target::interface::Target::create_or_update_user) for reference.
    async fn create_or_update_user(
        &mut self,
        source_user: std::sync::Arc<dyn crate::source::interface::User + Send + Sync>,
    ) -> Result<(), crate::error::KidsError> {
        tracing::info!(id = source_user.id(), "Create or update user");
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::target::interface::Target for Connector<super::external::ZammadClient> {
    type Config = ZammadConfig;

    async fn new(config: Self::Config) -> Result<Self, crate::error::KidsError> {
        let client = super::external::ZammadClient::new(config.zammad_api)
            .await
            .map_err(|e| e.with_context("Failed to create Synapse API client"))?;
        Ok(Connector { client })
    }

    fn info(&self) -> String {
        self.info()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), crate::error::KidsError> {
        self.full_sync_incoming().await
    }

    async fn all_groups(&mut self) -> Result<std::collections::HashSet<crate::types::SharedResourceIdentifier>, crate::error::KidsError> {
        self.all_groups().await
    }

    async fn all_users(&mut self) -> Result<std::collections::HashSet<crate::types::SharedResourceIdentifier>, crate::error::KidsError> {
        self.all_users().await
    }

    async fn delete_group(&mut self, source_group_id: &crate::types::SharedResourceIdentifier) -> Result<(), crate::error::KidsError> {
        self.delete_group(source_group_id).await
    }

    async fn delete_user(&mut self, user_id: &crate::types::SharedResourceIdentifier) -> Result<(), crate::error::KidsError> {
        self.delete_user(user_id).await
    }

    async fn create_or_update_group(
        &mut self,
        source_group: std::sync::Arc<dyn crate::source::interface::Group + Send + Sync>,
    ) -> Result<(), crate::error::KidsError> {
        self.create_or_update_group(source_group).await
    }

    async fn create_or_update_user(
        &mut self,
        source_user: std::sync::Arc<dyn crate::source::interface::User + Send + Sync>,
    ) -> Result<(), crate::error::KidsError> {
        self.create_or_update_user(source_user).await
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl crate::target::interface::Target for Connector<super::external::MockZammadApi> {
    type Config = ZammadConfig;

    async fn new(_config: Self::Config) -> Result<Self, crate::error::KidsError> {
        Ok(Connector {
            client: super::external::MockZammadApi::default(),
        })
    }

    fn info(&self) -> String {
        self.info()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), crate::error::KidsError> {
        self.full_sync_incoming().await
    }

    async fn all_groups(&mut self) -> Result<std::collections::HashSet<crate::types::SharedResourceIdentifier>, crate::error::KidsError> {
        self.all_groups().await
    }

    async fn all_users(&mut self) -> Result<std::collections::HashSet<crate::types::SharedResourceIdentifier>, crate::error::KidsError> {
        self.all_users().await
    }

    async fn delete_group(&mut self, source_group_id: &crate::types::SharedResourceIdentifier) -> Result<(), crate::error::KidsError> {
        self.delete_group(source_group_id).await
    }

    async fn delete_user(&mut self, user_id: &crate::types::SharedResourceIdentifier) -> Result<(), crate::error::KidsError> {
        self.delete_user(user_id).await
    }

    async fn create_or_update_group(
        &mut self,
        source_group: std::sync::Arc<dyn crate::source::interface::Group + Send + Sync>,
    ) -> Result<(), crate::error::KidsError> {
        self.create_or_update_group(source_group).await
    }

    async fn create_or_update_user(
        &mut self,
        source_user: std::sync::Arc<dyn crate::source::interface::User + Send + Sync>,
    ) -> Result<(), crate::error::KidsError> {
        self.create_or_update_user(source_user).await
    }
}

#[cfg(test)]
mod test {
    use super::*;

    type ZammadConnector = Connector<crate::target::zammad::external::MockZammadApi>;

    #[rstest::fixture]
    pub fn connector() -> ZammadConnector {
        Connector {
            client: crate::target::zammad::external::MockZammadApi::default(),
        }
    }

    #[rstest::rstest]
    fn info_works(connector: impl crate::target::interface::Target) {
        assert_eq!(connector.info(), "Zammad Connector!")
    }
}
