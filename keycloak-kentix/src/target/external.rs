#[derive(serde::Deserialize, Clone)]
pub struct KentixApiConfig {
    /// URL of the Kentix API endpoint, e.g. `https://kentix.example.com/`.
    pub kentix_url: url::Url,
    /// The [Usernames](super::dto::Username) of user we will ignore.
    ///
    /// These users are not managed by us, thus we never act on them actively or passively.
    ///
    /// You need to add the at least the user KIDS uses to this list, and potentially other
    /// admin accounts like the one you use to setup the levelprofiles.
    pub ignored_usernames: Vec<super::dto::Username>,
    /// The token used to access the Kentix API.
    pub bearer_token: String,
    /// Whether to validate the server certificate of the Matrix homeserver.
    /// Only disable for local development purposes!
    pub insecure_disable_tls_verification: bool,
    /// Path to the PEM of the root certificate to trust for Kentix.
    pub kentix_root_certificate_pem_path: Option<std::path::PathBuf>,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait KentixApi {
    async fn get_levelprofiles(&self) -> Result<Vec<super::dto::Levelprofile>, kids_lib::error::KidsError>;
    async fn get_users(&self) -> Result<Vec<super::dto::UserWithId>, kids_lib::error::KidsError>;
    async fn get_user(&self, user_id: super::dto::UserId) -> Result<super::dto::UserWithId, kids_lib::error::KidsError>;
    async fn create_user(&self, user: super::dto::User) -> Result<super::dto::UserWithId, kids_lib::error::KidsError>;
    async fn update_user(&self, user: super::dto::UserWithId) -> Result<super::dto::UserWithId, kids_lib::error::KidsError>;
    /// Returns the passed-in `user` in the error case.
    /// Otherwise, the user is consumed as it is deleted upstream.
    ///
    /// Note that the error variant is boxed to reduce the size of the [`Result`].
    /// See https://rust-lang.github.io/rust-clippy/master/index.html#result_large_err for more information.
    async fn delete_user(&self, user: super::dto::UserWithId) -> Result<(), Box<(super::dto::UserWithId, kids_lib::error::KidsError)>>;
}

pub struct KentixClient {
    config: KentixApiConfig,
    http_client: reqwest::Client,
}

impl KentixClient {
    /// `per_page` parameter for pagination.
    /// There is no way to fetch **all** data but 100k should be enough to overwhelm a Kentix device
    /// so much that there could never be more users in Kentix.
    const PER_PAGE: i32 = 100_000;
    pub fn new(config: KentixApiConfig) -> Self {
        tracing::info!(kentix_url=%config.kentix_url, "Connecting to Kentix");
        let http_client = {
            let mut builder = reqwest::Client::builder();
            if config.insecure_disable_tls_verification {
                tracing::warn!("Verification of Kentix server certificate is disabled. Do not use this setting in a production environment!");
                builder = builder.danger_accept_invalid_certs(true);
            }
            if let Some(kentix_root_certificate_pem_path) = config.kentix_root_certificate_pem_path.as_ref() {
                let kentix_root_certificate_pem =
                    std::fs::read(kentix_root_certificate_pem_path).expect("Cannot read PEM certificate at {kentix_root_certificate_pem_path}");
                builder = builder.add_root_certificate(
                    reqwest::Certificate::from_pem(kentix_root_certificate_pem.as_slice()).expect("Cannot get certificate from configured PEM."),
                );
            }
            builder.build().unwrap()
        };
        Self { config, http_client }
    }

    async fn paginated<T: serde::de::DeserializeOwned>(
        &self,
        response: impl std::future::Future<Output = Result<super::dto::PaginatedResponse<T>, kids_lib::error::KidsError>>,
    ) -> Result<Vec<T>, kids_lib::error::KidsError> {
        response.await.map(|paginated_response| {
            if let Ok(per_page) = paginated_response.meta.per_page.parse::<i32>() {
                if paginated_response.meta.total > per_page {
                    Err(kids_lib::error::KidsError::InternalError(
                        "There is more data than what was fetched! Increase PER_PAGE to continue using this client.".to_owned(),
                    ))
                } else {
                    Ok(paginated_response.data)
                }
            } else {
                Err(kids_lib::error::KidsError::InternalError("Expected a number as per_page.".to_owned()))
            }
        })?
    }

    async fn send_request_without_body<T: serde::de::DeserializeOwned + std::fmt::Debug>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
    ) -> Result<T, kids_lib::error::KidsError> {
        self.send_request(method, path, None::<serde_json::Value>).await
    }

    async fn send_request_with_body<B: serde::Serialize + std::fmt::Debug, T: serde::de::DeserializeOwned + std::fmt::Debug>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: B,
    ) -> Result<T, kids_lib::error::KidsError> {
        tracing::trace!(method = ?method, path = tracing::field::display(&path), body = ?body, json_body = ?serde_json::to_string(&body), "Performing request to Kentix");
        self.send_request(method, path, Some(body)).await
    }

    async fn send_request<B: serde::Serialize, T: serde::de::DeserializeOwned + std::fmt::Debug>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<T, kids_lib::error::KidsError> {
        let request = {
            let mut builder = self.http_client.request(method, format!("{}{}", self.config.kentix_url, path));
            if let Some(body) = body {
                builder = builder.json(&body);
            }
            builder = builder.bearer_auth(self.config.bearer_token.clone());
            builder = builder.header("Accept", "application/json");
            builder = builder.query(&[("per_page", Self::PER_PAGE)]);
            builder
        };
        tracing::trace!(request = ?request, "Performing request to Kentix");
        match request.send().await {
            Ok(response) => {
                tracing::trace!(response = ?response, "Received a response");
                let status = response.status();
                let url = response.url().to_string();
                if status.is_success() {
                    if status == http::StatusCode::NO_CONTENT {
                        return match serde_json::from_str("null") {
                            Ok(json) => {
                                tracing::trace!(result = "");
                                Ok(json)
                            }
                            Err(error) => Err(kids_lib::error::KidsError::ApiOperationFailed(
                                kids_lib::error::NO_CONTEXT.to_string(),
                                status.as_u16(),
                                url,
                                anyhow::anyhow!(error),
                            )),
                        };
                    }
                    return match response.json().await {
                        Ok(json) => {
                            tracing::trace!(result = ?json);
                            Ok(json)
                        }
                        Err(error) => Err(kids_lib::error::KidsError::ApiOperationFailed(
                            kids_lib::error::NO_CONTEXT.to_string(),
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
                    return Err(kids_lib::error::KidsError::AuthenticationFailed(
                        kids_lib::error::NO_CONTEXT.to_string(),
                        status.as_u16(),
                        url,
                        anyhow::anyhow!(error_information),
                    ));
                }

                Err(kids_lib::error::KidsError::ApiOperationFailed(
                    kids_lib::error::NO_CONTEXT.to_string(),
                    status.as_u16(),
                    url,
                    anyhow::anyhow!(error_information),
                ))
            }
            Err(e) => Err(kids_lib::error::KidsError::RequestFailed(
                kids_lib::error::NO_CONTEXT.to_string(),
                anyhow::anyhow!(e),
            )),
        }
    }
}

#[async_trait::async_trait]
impl KentixApi for KentixClient {
    async fn get_levelprofiles(&self) -> Result<Vec<super::dto::Levelprofile>, kids_lib::error::KidsError> {
        tracing::trace!("Getting all levelprofiles");
        self.send_request_without_body(http::Method::GET, kids_lib::types::ApiPath::from_segments(["api", "levelprofiles", "names"]))
            .await
    }
    async fn get_users(&self) -> Result<Vec<super::dto::UserWithId>, kids_lib::error::KidsError> {
        tracing::trace!("Getting all users");
        /// The `/api/users` response does not contain all necessary data.
        /// We therefore need this intermediate step to first fetch all IDs and from there fetch each user.
        #[derive(Debug, serde::Deserialize)]
        struct SparseUser {
            id: super::dto::UserId,
            username: super::dto::Username,
        }
        let user_ids = self
            .paginated(self.send_request_without_body(http::Method::GET, kids_lib::types::ApiPath::from_segments(["api", "users"])))
            .await?
            .into_iter()
            .filter(|user: &SparseUser| !self.config.ignored_usernames.contains(&user.username))
            .map(|user| user.id);
        let mut users = Vec::new();
        for user_id in user_ids {
            users.push(self.get_user(user_id).await?);
        }
        Ok(users)
    }
    async fn get_user(&self, user_id: super::dto::UserId) -> Result<super::dto::UserWithId, kids_lib::error::KidsError> {
        self.send_request_without_body(
            http::Method::GET,
            kids_lib::types::ApiPath::from_segments(["api", "users", &format!("{user_id}")]),
        )
        .await
    }
    async fn create_user(&self, user: super::dto::User) -> Result<super::dto::UserWithId, kids_lib::error::KidsError> {
        if self.config.ignored_usernames.contains(&user.username) {
            const ERROR_CONTEXT: &str = "Creating user";
            const ERROR_MSG: &str = "Cannot create ignored user.";
            tracing::error!(username = %user.username, "{ERROR_CONTEXT}: {ERROR_MSG}");
            return Err(kids_lib::error::KidsError::RequestFailed(
                ERROR_CONTEXT.to_owned(),
                anyhow::anyhow!("{ERROR_MSG}"),
            ));
        }
        tracing::trace!(username = user.username.0, "Creating user");
        self.send_request_with_body(http::Method::POST, kids_lib::types::ApiPath::from_segments(["api", "users"]), user)
            .await
    }
    async fn update_user(&self, user: super::dto::UserWithId) -> Result<super::dto::UserWithId, kids_lib::error::KidsError> {
        if self.config.ignored_usernames.contains(&user.user.username) {
            const ERROR_CONTEXT: &str = "Updating user";
            const ERROR_MSG: &str = "Cannot update ignored user.";
            tracing::error!(username = %user.user.username, "{ERROR_CONTEXT}: {ERROR_MSG}");
            return Err(kids_lib::error::KidsError::RequestFailed(
                ERROR_CONTEXT.to_owned(),
                anyhow::anyhow!("{ERROR_MSG}"),
            ));
        }
        tracing::trace!(username = user.user.username.0, "Updating user");
        self.send_request_with_body(
            http::Method::PATCH,
            kids_lib::types::ApiPath::from_segments(["api", "users", &format!("{}", user.id)]),
            user,
        )
        .await
    }
    async fn delete_user(&self, user: super::dto::UserWithId) -> Result<(), Box<(super::dto::UserWithId, kids_lib::error::KidsError)>> {
        if self.config.ignored_usernames.contains(&user.user.username) {
            const ERROR_CONTEXT: &str = "Updating user";
            const ERROR_MSG: &str = "Cannot update ignored user.";
            tracing::error!(username = %user.user.username, "{ERROR_CONTEXT}: {ERROR_MSG}");
            return Err(Box::new((
                user,
                kids_lib::error::KidsError::RequestFailed(ERROR_CONTEXT.to_owned(), anyhow::anyhow!("{ERROR_MSG}")),
            )));
        }
        let user_id = user.id;
        tracing::trace!(id = user_id.0, "Deleting user");
        match self
            .send_request_without_body(
                http::Method::DELETE,
                kids_lib::types::ApiPath::from_segments(["api", "users", &format!("{user_id}")]),
            )
            .await
        {
            Err(err) => Err(Box::new((user, err))),
            Ok(result) => Ok(result),
        }
    }
}
