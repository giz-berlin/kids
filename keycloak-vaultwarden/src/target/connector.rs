use std::collections;
use std::sync::Arc;

use kids_lib::error::KidsError;
use kids_lib::types::SharedResourceIdentifier;

use crate::target::external::admin::AdminClient;
use crate::target::external::cli::CliClient;
use crate::target::external::directory_api::DirectoryApiClient;
use crate::target::external::http::HttpConfig;
use crate::target::external::http::auth::ApiKey;
use crate::target::external::vault_api::VaultApiClient;
use crate::target::interactor::{Apis, VaultwardenInteractor};

#[derive(serde::Deserialize)]
pub struct VaultwardenConfig {
    /// URL of the Vaultwarden server.
    url: url::Url,
    /// ID of the organization to sync to.
    org_id: String,

    /// API key of an owner or admin of the organization.
    user_client_id: String,
    user_client_secret: String,
    /// Master password of the same user.
    master_password: String,

    /// API key of the organization itself.
    org_client_id: String,
    org_client_secret: String,

    /// Value of the `ADMIN_TOKEN` configured on the Vaultwarden server.
    admin_token: String,

    /// Only users who have this role are given access to Vaultwarden.
    /// When this is not present, all users are given access.
    required_role_name: Option<String>,

    /// Emails of accounts the syncer should never touch.
    ignored_emails: Vec<String>,

    /// Whether to validate the server certificate of the Vaultwarden server.
    /// Only disable for local development purposes!
    insecure_disable_tls_verification: bool,
}

/// A connector to Vaultwarden providing the [Target](kids_lib::interface::target::Target) interface.
///
/// Every source group becomes a Vaultwarden group and a collection, both named after the group's path
/// (such as `parent/child`). Every source user with access becomes a member of the organization
/// and of the Vaultwarden groups of their direct source groups.
pub struct Connector {
    required_role_name: Option<String>,
    interactor: VaultwardenInteractor,
}

impl Connector {
    async fn connect(config: &VaultwardenConfig) -> Result<Apis, KidsError> {
        let http = HttpConfig {
            url: config.url.clone(),
            insecure_disable_tls_verification: config.insecure_disable_tls_verification,
        };
        let user_api_key = ApiKey {
            client_id: config.user_client_id.clone(),
            client_secret: config.user_client_secret.clone(),
        };
        let org_api_key = ApiKey {
            client_id: config.org_client_id.clone(),
            client_secret: config.org_client_secret.clone(),
        };

        Ok(Apis {
            admin: AdminClient::new(&http, config.admin_token.clone())
                .await
                .map_err(|e| e.with_context("Failed to create admin client"))?,
            vault: VaultApiClient::new(&http, config.org_id.clone(), user_api_key.clone())
                .await
                .map_err(|e| e.with_context("Failed to create vault API client"))?,
            directory: DirectoryApiClient::new(&http, org_api_key)
                .await
                .map_err(|e| e.with_context("Failed to create directory API client"))?,
            cli: CliClient::new(http, config.org_id.clone(), user_api_key, config.master_password.clone())
                .await
                .map_err(|e| e.with_context("Failed to create bw CLI client"))?,
        })
    }

    async fn has_access(&self, source_user: &(dyn kids_lib::interface::source::User + Send + Sync)) -> Result<bool, KidsError> {
        if !source_user.enabled() {
            return Ok(false);
        }
        match &self.required_role_name {
            Some(role) => Ok(source_user.client_roles().await?.contains(role)),
            None => Ok(true),
        }
    }
}

#[async_trait::async_trait]
impl kids_lib::interface::target::Target for Connector {
    type Config = VaultwardenConfig;

    async fn new(config: Self::Config) -> Result<Self, KidsError> {
        let apis = Self::connect(&config).await?;
        let interactor = VaultwardenInteractor::new(apis, config.ignored_emails).await?;
        Ok(Self {
            required_role_name: config.required_role_name,
            interactor,
        })
    }

    fn info(&self) -> String {
        "Vaultwarden Connector!".to_string()
    }

    async fn full_sync_incoming(&mut self) -> Result<(), KidsError> {
        tracing::info!("To prepare for full sync, re-fetching the current state of Vaultwarden");
        self.interactor.refresh().await
    }

    async fn all_groups(&mut self) -> Result<collections::HashSet<SharedResourceIdentifier>, KidsError> {
        Ok(self
            .interactor
            .group_ids()
            .iter()
            .chain(self.interactor.collection_ids().iter())
            .cloned()
            .collect())
    }

    async fn all_users(&mut self) -> Result<collections::HashSet<SharedResourceIdentifier>, KidsError> {
        Ok(self.interactor.user_ids())
    }

    async fn delete_group(&mut self, group_id: &SharedResourceIdentifier) -> Result<(), KidsError> {
        self.interactor.delete_group(group_id).await
    }

    async fn delete_user(&mut self, user_id: &SharedResourceIdentifier) -> Result<(), KidsError> {
        self.interactor.delete_user(user_id).await
    }

    async fn create_or_update_group(&mut self, source_group: Arc<dyn kids_lib::interface::source::Group + Send + Sync>) -> Result<(), KidsError> {
        let name = source_group.path().trim_start_matches('/');
        self.interactor.ensure_group(source_group.id(), name).await
    }

    async fn create_or_update_user(&mut self, source_user: Arc<dyn kids_lib::interface::source::User + Send + Sync>) -> Result<(), KidsError> {
        if !self.has_access(source_user.as_ref()).await? {
            return self.interactor.ensure_user_inactive(source_user.id(), source_user.email()).await;
        }
        // groups(false) = don't include parent groups, groups(true) = include parent groups
        // if users should be added to all parents groups of a group they're member of in
        // keycloak, change this to groups(true)
        let group_ids = source_user.groups(false).await?.iter().map(|group| group.id().clone()).collect();
        self.interactor.ensure_user_active(source_user.id(), source_user.email(), &group_ids).await
    }
}
