#[derive(serde::Deserialize)]
pub struct ZammadApiConfig {
    base_url: url::Url,
    syncer_username: String,
    syncer_password: String,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ZammadApi {
    async fn user_set_roles<RoleIterator: Iterator<Item = crate::target::zammad::types::RoleId> + 'static + std::marker::Send>(
        &self,
        user_id: super::types::UserId,
        roles: RoleIterator,
    ) -> Result<super::types::User, crate::error::KidsError>;

    async fn enable_user(&self, user_id: super::types::UserId) -> Result<super::types::User, crate::error::KidsError>;
    async fn disable_user(&self, user_id: super::types::UserId) -> Result<super::types::User, crate::error::KidsError>;
}

pub struct ZammadClient {
    config: ZammadApiConfig,
    http_client: reqwest::Client,
}

impl ZammadClient {
    pub async fn new(config: ZammadApiConfig) -> Result<Self, crate::error::KidsError> {
        let mut builder = reqwest::Client::builder();
        // if config.insecure_disable_tls_verification {
        //     tracing::warn!("Verification of Matrix server certificate is disabled. Do not use this setting in a production environment!");
        //     builder = builder.danger_accept_invalid_certs(true);
        // }
        let client = builder.build().unwrap();
        Ok(Self { config, http_client: client })
    }

    fn construct_request<B: serde::Serialize>(&self, method: http::Method, url: String, body: Option<B>) -> reqwest::RequestBuilder {
        let mut builder = self.http_client.request(method, &url);
        if let Some(body) = body {
            builder = builder.json(&body)
        }
        builder = builder.basic_auth(self.config.syncer_username.clone(), Some(self.config.syncer_password.clone()));
        builder
    }

    async fn send_request<T: serde::de::DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T, crate::error::KidsError> {
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let url = response.url().to_string();
                if status.is_success() {
                    return match response.json().await {
                        Ok(json) => Ok(json),
                        Err(error) => Err(crate::error::KidsError::ApiOperationFailed(
                            crate::error::NO_CONTEXT.to_string(),
                            status.as_u16(),
                            url,
                            anyhow::anyhow!(error),
                        )),
                    };
                }

                let error_information = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to obtain error information from response text".to_string());

                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Err(crate::error::KidsError::AuthenticationFailed(
                        crate::error::NO_CONTEXT.to_string(),
                        status.as_u16(),
                        url,
                        anyhow::anyhow!(error_information),
                    ));
                }

                Err(crate::error::KidsError::ApiOperationFailed(
                    crate::error::NO_CONTEXT.to_string(),
                    status.as_u16(),
                    url,
                    anyhow::anyhow!(error_information),
                ))
            }
            Err(e) => Err(crate::error::KidsError::RequestFailed(crate::error::NO_CONTEXT.to_string(), anyhow::anyhow!(e))),
        }
    }

    async fn make_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: http::Method,
        path: String,
        body: Option<B>,
    ) -> Result<T, crate::error::KidsError> {
        let request = self.construct_request(method, format!("{}api/v1/{}", self.config.base_url, path), body);
        self.send_request(request).await
    }

    async fn user_set_active(&self, user_id: super::types::UserId, active: super::types::UserActive) -> Result<super::types::User, crate::error::KidsError> {
        let mut body = serde_json::Map::new();
        body.insert("active".to_owned(), active.into());
        self.make_request::<_, super::types::User>(http::Method::PUT, format!("users/{user_id}"), Some(body))
            .await
    }
}

#[async_trait::async_trait]
impl ZammadApi for ZammadClient {
    async fn user_set_roles<RoleIterator: Iterator<Item = super::types::RoleId> + 'static + std::marker::Send>(
        &self,
        user_id: super::types::UserId,
        roles: RoleIterator,
    ) -> Result<super::types::User, crate::error::KidsError> {
        let mut body = serde_json::Map::new();
        body.insert("roles".to_owned(), roles.collect());
        self.make_request::<_, super::types::User>(http::Method::PUT, format!("users/{user_id}"), Some(body))
            .await
    }

    async fn enable_user(&self, user_id: super::types::UserId) -> Result<super::types::User, crate::error::KidsError> {
        self.user_set_active(user_id, true.into()).await
    }

    async fn disable_user(&self, user_id: super::types::UserId) -> Result<super::types::User, crate::error::KidsError> {
        self.user_set_active(user_id, false.into()).await
    }
}
