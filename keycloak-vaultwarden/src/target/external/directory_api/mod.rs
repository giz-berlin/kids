//! Client for the directory import endpoint, the only endpoint that sets the external ID of a member.
//!
//! Vaultwarden matches imported members to existing ones by email: an existing member gets the
//! external ID assigned, and an email without an account gets a new account and an invitation.

mod dto;

use kids_lib::error::KidsError;
use kids_lib::types::ApiPath;

use crate::target::external::http::auth::{ApiKey, Scope, TokenAuthenticator};
use crate::target::external::http::{ApiClient, HttpConfig};

pub struct DirectoryApiClient {
    transport: ApiClient<TokenAuthenticator>,
}

impl DirectoryApiClient {
    pub async fn new(http: &HttpConfig, org_api_key: ApiKey) -> Result<Self, KidsError> {
        let transport = ApiClient::new(http, "api", TokenAuthenticator::new(org_api_key, Scope::Organization)).await?;
        Ok(Self { transport })
    }

    /// Invites the email to the organization, or restores its membership if it was revoked.
    pub async fn import_member(&self, email: &str, external_id: &str) -> Result<(), KidsError> {
        self.import(email, external_id, false)
            .await
            .map_err(|error| error.with_context(&format!("Importing member '{email}' (external ID {external_id})")))?;
        tracing::info!(email, external_id, "Imported member");
        Ok(())
    }

    /// Revokes the membership of the email. Vaultwarden never revokes the last owner of the organization.
    pub async fn revoke_member(&self, email: &str, external_id: &str) -> Result<(), KidsError> {
        self.import(email, external_id, true)
            .await
            .map_err(|error| error.with_context(&format!("Revoking member '{email}' (external ID {external_id})")))?;
        tracing::info!(email, external_id, "Revoked member");
        Ok(())
    }

    async fn import(&self, email: &str, external_id: &str, deleted: bool) -> Result<(), KidsError> {
        let member = dto::ImportMember { email, external_id, deleted };
        self.transport
            .request_discarding_response(
                http::Method::POST,
                ApiPath::from_segments(["public", "organization", "import"]),
                Some(dto::ImportRequest {
                    groups: &[],
                    members: &[member],
                    overwrite_existing: false,
                }),
                &[],
            )
            .await
    }
}
