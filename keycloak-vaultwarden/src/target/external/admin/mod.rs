mod dto;

pub use dto::User;

use kids_lib::error::KidsError;
use kids_lib::types::ApiPath;

use crate::target::external::http::auth::AdminSessionAuthenticator;
use crate::target::external::http::{ApiClient, HttpConfig};

/// Rocket only routes POST requests with a JSON content type to the user actions, as they
/// declare `format = "application/json"`, even though they take no body.
const JSON_CONTENT_TYPE: &[(http::HeaderName, &str)] = &[(http::header::CONTENT_TYPE, "application/json")];

pub struct AdminClient {
    transport: ApiClient<AdminSessionAuthenticator>,
}

impl AdminClient {
    pub async fn new(http: &HttpConfig, admin_token: String) -> Result<Self, KidsError> {
        let transport = ApiClient::new(http, "admin", AdminSessionAuthenticator::new(admin_token)).await?;
        Ok(Self { transport })
    }

    pub async fn get_users(&self) -> Result<Vec<User>, KidsError> {
        self.transport
            .request(http::Method::GET, ApiPath::from_segments(["users"]), None::<()>)
            .await
            .map_err(|error| error.with_context("Fetching all users"))
    }

    pub async fn get_user(&self, user_id: &str) -> Result<Option<User>, KidsError> {
        self.transport
            .request_optional(http::Method::GET, ApiPath::from_segments(["users", user_id]), None::<()>)
            .await
            .map_err(|error| error.with_context(&format!("Fetching user {user_id}")))
    }

    pub async fn enable_user(&self, user_id: &str) -> Result<(), KidsError> {
        self.post_user_action(user_id, "enable")
            .await
            .map_err(|error| error.with_context(&format!("Enabling user {user_id}")))?;
        tracing::info!(user_id, "Enabled user");
        Ok(())
    }

    pub async fn disable_user(&self, user_id: &str) -> Result<(), KidsError> {
        self.post_user_action(user_id, "disable")
            .await
            .map_err(|error| error.with_context(&format!("Disabling user {user_id}")))?;
        tracing::info!(user_id, "Disabled user");
        Ok(())
    }

    /// Deletes the account including its personal vault. Also succeeds if the account does not exist.
    pub async fn delete_user(&self, user_id: &str) -> Result<(), KidsError> {
        self.transport
            .request_ignoring_not_found(
                http::Method::POST,
                ApiPath::from_segments(["users", user_id, "delete"]),
                None::<()>,
                JSON_CONTENT_TYPE,
            )
            .await
            .map_err(|error| error.with_context(&format!("Deleting user {user_id}")))?;
        tracing::info!(user_id, "Deleted user");
        Ok(())
    }

    async fn post_user_action(&self, user_id: &str, action: &str) -> Result<(), KidsError> {
        self.transport
            .request_discarding_response(
                http::Method::POST,
                ApiPath::from_segments(["users", user_id, action]),
                None::<()>,
                JSON_CONTENT_TYPE,
            )
            .await
    }
}
