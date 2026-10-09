use reqwest::RequestBuilder;

use kids_lib::error::KidsError;

/// Attaches credentials to the requests of an [`ApiClient`](super::ApiClient).
#[async_trait::async_trait]
pub trait Authenticator: Send + Sync {
    /// Whether the credentials are a session cookie that the HTTP client has to keep.
    const USES_SESSION_COOKIE: bool = false;

    /// Attaches valid credentials to `request`, obtaining them first if none are cached yet.
    async fn prepare(&self, request: RequestBuilder, http_client: &reqwest::Client, base_url: &url::Url) -> Result<RequestBuilder, KidsError>;

    /// Obtains new credentials. Called on client construction and when the server rejects the current ones.
    async fn refresh(&self, http_client: &reqwest::Client, base_url: &url::Url) -> Result<(), KidsError>;
}

#[derive(Clone)]
pub struct ApiKey {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Clone, Copy)]
pub enum Scope {
    User,
    Organization,
}

impl Scope {
    fn as_str(self) -> &'static str {
        match self {
            Scope::User => "api",
            Scope::Organization => "api.organization",
        }
    }
}

const TOKEN_PATH: &str = "identity/connect/token";
/// Device type `21` is `SDK`, the same the directory connector and the CLI use.
const DEVICE_TYPE: &str = "21";
const DEVICE_NAME: &str = "kids-vaultwarden-syncer";

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    /// Lifetime of the access token in seconds.
    expires_in: i64,
}

struct Token {
    access_token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl Token {
    fn is_expired(&self) -> bool {
        // Renew slightly early, so that the token cannot expire while a request is in flight.
        self.expires_at - chrono::Duration::seconds(5) < chrono::Utc::now()
    }
}

impl From<TokenResponse> for Token {
    fn from(value: TokenResponse) -> Self {
        Self {
            access_token: value.access_token,
            expires_at: chrono::Utc::now() + chrono::Duration::seconds(value.expires_in),
        }
    }
}

/// Authenticates with a bearer token obtained through the OAuth2 client credentials flow.
pub struct TokenAuthenticator {
    api_key: ApiKey,
    scope: Scope,
    token: tokio::sync::Mutex<Option<Token>>,
}

impl TokenAuthenticator {
    pub fn new(api_key: ApiKey, scope: Scope) -> Self {
        Self {
            api_key,
            scope,
            token: tokio::sync::Mutex::new(None),
        }
    }

    /// Derived from the client ID, so that Vaultwarden recognizes every login as the same device
    /// instead of registering a new one each time.
    fn device_identifier(&self) -> String {
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, self.api_key.client_id.as_bytes()).to_string()
    }

    async fn login(&self, http_client: &reqwest::Client, base_url: &url::Url) -> Result<Token, KidsError> {
        let device_identifier = self.device_identifier();
        let request = http_client.post(super::join(base_url, TOKEN_PATH)?).form(&[
            ("grant_type", "client_credentials"),
            ("client_id", self.api_key.client_id.as_str()),
            ("client_secret", self.api_key.client_secret.as_str()),
            ("scope", self.scope.as_str()),
            ("device_identifier", device_identifier.as_str()),
            ("device_name", DEVICE_NAME),
            ("device_type", DEVICE_TYPE),
        ]);
        let response = super::send(request).await?;
        let token_response: TokenResponse = super::parse_json(response).await.map_err(|error| error.with_context("Fetching access token"))?;

        tracing::debug!(scope = self.scope.as_str(), "Fetched access token");
        Ok(token_response.into())
    }
}

#[async_trait::async_trait]
impl Authenticator for TokenAuthenticator {
    async fn prepare(&self, request: RequestBuilder, http_client: &reqwest::Client, base_url: &url::Url) -> Result<RequestBuilder, KidsError> {
        let mut cached_token = self.token.lock().await;
        let token = match cached_token.take() {
            Some(token) if !token.is_expired() => token,
            _ => self.login(http_client, base_url).await?,
        };
        let request = request.bearer_auth(&token.access_token);
        *cached_token = Some(token);
        Ok(request)
    }

    async fn refresh(&self, http_client: &reqwest::Client, base_url: &url::Url) -> Result<(), KidsError> {
        let mut cached_token = self.token.lock().await;
        *cached_token = Some(self.login(http_client, base_url).await?);
        Ok(())
    }
}

const ADMIN_LOGIN_PATH: &str = "admin";
const ADMIN_LOGIN_CONTEXT: &str = "Logging in to admin panel";

/// Authenticates with the session cookie of the admin panel, obtained by logging in with the `ADMIN_TOKEN`.
pub struct AdminSessionAuthenticator {
    admin_token: String,
}

impl AdminSessionAuthenticator {
    pub fn new(admin_token: String) -> Self {
        Self { admin_token }
    }
}

#[async_trait::async_trait]
impl Authenticator for AdminSessionAuthenticator {
    const USES_SESSION_COOKIE: bool = true;

    async fn prepare(&self, request: RequestBuilder, _http_client: &reqwest::Client, _base_url: &url::Url) -> Result<RequestBuilder, KidsError> {
        // The HTTP client attaches the session cookie.
        Ok(request)
    }

    async fn refresh(&self, http_client: &reqwest::Client, base_url: &url::Url) -> Result<(), KidsError> {
        let request = http_client.post(super::join(base_url, ADMIN_LOGIN_PATH)?).form(&[("token", &self.admin_token)]);
        let response = super::send(request).await.map_err(|error| error.with_context(ADMIN_LOGIN_CONTEXT))?;
        if !response.status().is_success() {
            return Err(super::response_error(response).await.with_context(ADMIN_LOGIN_CONTEXT));
        }
        Ok(())
    }
}
