pub mod auth;

use anyhow::anyhow;
use reqwest::StatusCode;

use kids_lib::error::KidsError;

pub struct HttpConfig {
    pub url: url::Url,
    pub insecure_disable_tls_verification: bool,
}

/// An HTTP client for one Vaultwarden API, authenticating its requests with `A`.
pub struct ApiClient<A: auth::Authenticator> {
    auth: A,
    base_url: url::Url,
    http_client: reqwest::Client,
    path_prefix: &'static str,
}

impl<A: auth::Authenticator> ApiClient<A> {
    /// Builds the client and obtains initial credentials.
    pub async fn new(config: &HttpConfig, path_prefix: &'static str, auth: A) -> Result<Self, KidsError> {
        let mut builder = reqwest::Client::builder().cookie_store(A::USES_SESSION_COOKIE);
        if config.insecure_disable_tls_verification {
            tracing::warn!("Verification of the Vaultwarden server certificate is disabled. Do not use this setting in a production environment!");
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http_client = builder
            .build()
            .map_err(|error| KidsError::InternalError(anyhow::anyhow!(format!("Failed to build HTTP client: {error}"))))?;

        // Without a trailing slash, joining a relative path would replace the last path segment of the server URL.
        let mut base_url = config.url.clone();
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }

        let client = Self {
            auth,
            base_url,
            http_client,
            path_prefix,
        };
        client.auth.refresh(&client.http_client, &client.base_url).await?;
        Ok(client)
    }

    pub async fn request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<T, KidsError> {
        let response = self.send_authenticated(method, path, body, &[]).await?;
        parse_json(response).await
    }

    /// Like [`Self::request`], but maps a `404 Not Found` response to `Ok(None)`.
    pub async fn request_optional<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<Option<T>, KidsError> {
        let response = self.send_authenticated(method, path, body, &[]).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        parse_json(response).await.map(Some)
    }

    pub async fn request_discarding_response<B: serde::Serialize>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
        headers: &[(http::HeaderName, &str)],
    ) -> Result<(), KidsError> {
        let response = self.send_authenticated(method, path, body, headers).await?;
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }
        Ok(())
    }

    /// Like [`Self::request_discarding_response`], but treats `404 Not Found` as success.
    pub async fn request_ignoring_not_found<B: serde::Serialize>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
        headers: &[(http::HeaderName, &str)],
    ) -> Result<(), KidsError> {
        let response = self.send_authenticated(method, path, body, headers).await?;
        if !response.status().is_success() && response.status() != StatusCode::NOT_FOUND {
            return Err(response_error(response).await);
        }
        Ok(())
    }

    /// Sends the request, renewing the credentials and retrying once if the server rejects them.
    async fn send_authenticated<B: serde::Serialize>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
        headers: &[(http::HeaderName, &str)],
    ) -> Result<reqwest::Response, KidsError> {
        let url = join(&self.base_url, &format!("{}/{path}", self.path_prefix))?;
        let mut request = self.http_client.request(method, url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        // Cloned before authenticating, so that the retry is authenticated with the renewed credentials.
        let retry_request = request.try_clone().expect("only requests with a streaming body cannot be cloned");

        let response = send(self.auth.prepare(request, &self.http_client, &self.base_url).await?).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }

        tracing::debug!("Credentials were rejected, renewing them and retrying the request");
        self.auth.refresh(&self.http_client, &self.base_url).await?;
        send(self.auth.prepare(retry_request, &self.http_client, &self.base_url).await?).await
    }
}

fn join(base_url: &url::Url, path: &str) -> Result<url::Url, KidsError> {
    base_url
        .join(path)
        .map_err(|error| KidsError::InternalError(anyhow::anyhow!(format!("Cannot join '{path}' to '{base_url}': {error}"))))
}

async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, KidsError> {
    request
        .send()
        .await
        .map_err(|error| KidsError::RequestFailed(kids_lib::error::no_context().to_string(), anyhow!(error)))
}

async fn parse_json<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, KidsError> {
    let status = response.status();
    if !status.is_success() {
        return Err(response_error(response).await);
    }
    let url = response.url().to_string();
    response
        .json()
        .await
        .map_err(|error| KidsError::ApiOperationFailed(kids_lib::error::no_context().to_string(), status.as_u16(), url, anyhow!(error)))
}

async fn response_error(response: reqwest::Response) -> KidsError {
    let status = response.status();
    let url = response.url().to_string();
    let error_information = response
        .text()
        .await
        .unwrap_or_else(|_| "Failed to obtain error information from response text".to_string());

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return KidsError::AuthenticationFailed(kids_lib::error::no_context().to_string(), status.as_u16(), url, anyhow!(error_information));
    }
    KidsError::ApiOperationFailed(kids_lib::error::no_context().to_string(), status.as_u16(), url, anyhow!(error_information))
}
