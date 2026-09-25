use anyhow::anyhow;
use reqwest::RequestBuilder;

use kids_lib::error::KidsError;

use crate::target::dto;

#[derive(serde::Deserialize, Clone)]
pub struct ApiAccessConfig {
    /// Client ID to authenticate against for MAS routes (`api/admin/v1`).
    pub mas_client_id: String,
    /// Client Secret to authenticate with for MAS routes (`api/admin/v1`).
    pub mas_client_secret: String,
    /// How many seconds tokens for Matrix and Synapse endpoint should be valid.
    pub token_validity_seconds: u32,
}

#[derive(serde::Deserialize, Clone)]
pub struct SynapseApiConfig {
    /// URL of the Matrix Authentication Service (probably similar to https://matrix.example.com/auth).
    pub matrix_mas_url: String,
    /// URL of the Matrix homeserver (probably similar to https://matrix.example.com).
    pub matrix_homeserver_url: String,
    /// The ULID of the source (for example, Keycloak) as identity provider in the MAS config.
    /// E.g. `01M2FZGR28JRY6XHVB574A83S0`.
    pub matrix_source_oidc_provider_ulid: String,
    /// User ID of a Matrix user that must have administrative access.
    /// This user will perform all operations required for syncing users and rooms.
    /// Create a dedicated user if you can.
    /// (probably similar to `@keycloak-sync:matrix.example.org`)
    pub matrix_syncer_user_id: crate::target::types::MatrixUserId,
    /// Tokens for the syncer Matrix user.
    pub api_access: ApiAccessConfig,
    /// Whether to validate the server certificate of the Matrix homeserver.
    /// Only disable for local development purposes!
    pub insecure_disable_tls_verification: bool,
    /// LEGACY: Needed for getting account data events used by old matrix syncer.
    /// Only needed while migrating from old syncer, should be removed afterward.
    pub matrix_namespace: String,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait SynapseApi {
    fn user_is_matrix_syncer(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> bool;
    fn homeserver_domain(&self) -> &crate::target::types::MatrixServerName;
    async fn get_joined_rooms_of_syncer(&self) -> Result<dto::matrix::JoinedRoomsResponse, KidsError>;
    async fn syncer_leave_room(&self, matrix_room_id: &str) -> Result<(), KidsError>;
    async fn get_mas_users(&self) -> Result<Vec<dto::mas::UserResponse>, KidsError>;
    async fn get_user_emails(&self, mas_user_id: &dto::mas::internal::user::Id) -> Result<Vec<String>, KidsError>;
    async fn set_user_emails(&self, mas_user_id: &dto::mas::internal::user::Id, emails: &[String]) -> Result<(), KidsError>;
    async fn lock_user(&self, mas_user_id: &dto::mas::internal::user::Id) -> Result<(), KidsError>;
    async fn unlock_user(&self, mas_user_id: &dto::mas::internal::user::Id) -> Result<(), KidsError>;
    async fn set_user_display_name(&self, matrix_user_id: &crate::target::types::MatrixUserId, display_name: &str) -> Result<(), KidsError>;
    async fn get_user_display_name(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<Option<String>, KidsError>;
    async fn set_admin_status(&self, mas_user_id: &dto::mas::internal::user::Id, should_be_admin: bool) -> Result<(), KidsError>;
    async fn create_user(
        &self,
        matrix_user_id: &crate::target::types::MatrixUserId,
        source_user_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<dto::mas::UserResponse, KidsError>;
    async fn mas_user_for_matrix_user_id(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<dto::mas::UserResponse, KidsError>;
    async fn associate_source_user_id_to_user(
        &self,
        matrix_user_id: &crate::target::types::MatrixUserId,
        source_user_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<(), KidsError>;

    async fn create_room(&self, name: &str, path: &str) -> Result<dto::matrix::RoomCreationResponse, KidsError>;
    async fn delete_room(&self, matrix_room_id: &str) -> Result<(), KidsError>;
    async fn associate_source_group_id_to_room(
        &self,
        matrix_room_id: &str,
        source_group_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<(), KidsError>;
    async fn get_room_associated_source_group_id(&self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError>;
    async fn get_room_associated_source_group_id_v1(&self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError>;
    async fn set_room_display_name(&self, matrix_room_id: &str, display_name: &str) -> Result<(), KidsError>;
    async fn get_room_display_name(&self, matrix_room_id: &str) -> Result<String, KidsError>;

    fn full_room_alias(&self, group_path: &str) -> String;
    async fn create_room_alias(&self, matrix_room_id: &str, alias: &str) -> Result<(), KidsError>;
    async fn delete_room_alias(&self, alias: &str) -> Result<(), KidsError>;
    async fn set_room_canonical_alias(&self, matrix_room_id: &str, canonical_alias: &str) -> Result<(), KidsError>;
    async fn get_room_canonical_alias(&self, matrix_room_id: &str) -> Result<dto::matrix::RoomCanonicalAliasEvent, KidsError>;

    /// Get the source user id as stored by MAS via the upstream Oauth provider.
    async fn get_source_user_id_for_mas_user_id(
        &self,
        mas_user_id: &dto::mas::internal::user::Id,
    ) -> Result<Option<kids_lib::types::SharedResourceIdentifier>, KidsError>;
    async fn get_user_joined_rooms(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<dto::matrix::UserJoinedRoomsResponse, KidsError>;
    async fn get_room_joined_users(&self, matrix_room_id: &str) -> Result<dto::matrix::RoomJoinedUsersResponse, KidsError>;
    async fn join_user_to_room(&self, matrix_room_id: &str, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<(), KidsError>;
    async fn kick_user_from_room(&self, matrix_room_id: &str, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<(), KidsError>;
}

pub struct ApiAccess {
    access_tokens: tokio::sync::Mutex<std::collections::HashMap<crate::target::types::AccessTokenScope, crate::target::types::AccessToken>>,
    /// [Account](oidc_rp::account::Account) used for MAS routes (`api/admin/v1`).
    mas_account: oidc_rp::account::Account<
        oidc_rp::oidc::EmptyAdditionalClaims,
        oidc_rp::oidc::EmptyAdditionalClaims,
        oidc_rp::oidc::EmptyAdditionalProviderMetadata,
        oidc_rp::account::access_token_type::Opaque,
        oidc_rp::account::account_user_type::ServiceAccount,
        oidc_rp::types::AttributeSet,
        oidc_rp::types::AttributeSet,
    >,
}

pub struct SynapseClient {
    api_access: ApiAccess,
    config: SynapseApiConfig,
    http_client: reqwest::Client,
    parsed_mas_url: url::Url,
    parsed_homeserver_url: url::Url,
}

/// The name of the room state event the syncer stores its metadata in
/// (such as the mapping of room to source group).
const SYNCER_ROOM_METADATA_EVENT: &str = "m.room.kids.room_sync";

fn error_to_kids_error(err: impl Into<anyhow::Error>) -> kids_lib::error::KidsError {
    let err = err.into();
    err.into()
}

impl SynapseClient {
    pub async fn new(config: SynapseApiConfig) -> Result<Self, KidsError> {
        let parsed_mas_url = url::Url::parse(&config.matrix_mas_url).expect("MAS URL should be parseable");
        let parsed_homeserver_url = url::Url::parse(&config.matrix_homeserver_url).expect("Homeserver URL should be parseable");
        tracing::info!(homeserver_url=%parsed_homeserver_url, mas_url=%parsed_mas_url, "Connecting to homeserver");

        let mut builder = reqwest::Client::builder();
        if config.insecure_disable_tls_verification {
            tracing::warn!("Verification of Matrix server certificate is disabled. Do not use this setting in a production environment!");
            builder = builder.danger_accept_invalid_certs(true);
        }
        let client = builder.build().unwrap();

        let mas_account = {
            let idp = if config.insecure_disable_tls_verification {
                oidc_rp::idp::IdP::new_with_reqwest_client(parsed_mas_url.clone(), client.clone()).await
            } else {
                oidc_rp::idp::IdP::new(parsed_mas_url.clone()).await
            }
            .map_err(error_to_kids_error)?
            .set_default_idp_refresh_strategy()
            .await
            .map_err(error_to_kids_error)?;
            let verifier = oidc_rp::verifier::Verifier::<oidc_rp::oidc::EmptyAdditionalClaims>::new(idp.clone(), config.api_access.mas_client_id.clone())
                .map_err(error_to_kids_error)?
                .set_access_token_allowed_jose_types(vec![
                    oidc_rp::oidc::JsonWebTokenType::new("JWT".to_owned())
                        .normalize()
                        .map_err(error_to_kids_error)?,
                ])
                .allow_other_audiences();
            let account = oidc_rp::account::Account::from_secret_client(
                idp,
                config.api_access.mas_client_id.clone(),
                config.api_access.mas_client_secret.clone(),
                verifier,
            );
            let account = account
                .set_scopes(vec!["urn:mas:admin".to_owned()])
                .exchange_client_credentials()
                .await
                .map_err(error_to_kids_error)?;
            account.start_auto_refresh()
        };
        let api_access = ApiAccess {
            access_tokens: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            mas_account,
        };
        let synapse_client = SynapseClient {
            config,
            http_client: client,
            api_access,
            parsed_mas_url,
            parsed_homeserver_url,
        };

        Ok(synapse_client)
    }

    async fn new_admin_personal_session(
        &self,
        scope: &crate::target::types::AccessTokenScope,
    ) -> Result<dto::mas::PersonalSessionResponse, kids_lib::error::KidsError> {
        let validity = chrono::Duration::seconds(self.config.api_access.token_validity_seconds as i64);
        let admin_id = self.mas_user_for_matrix_user_id(&self.config.matrix_syncer_user_id).await?.id;
        tracing::debug!(
            actor_id = self.config.matrix_syncer_user_id.display(),
            validity = validity.to_string(),
            scope = scope.0,
            "Generating a new admin personal session"
        );
        let validity_seconds = validity.num_seconds();
        Ok(self
            .send_mas_admin_request_single(
                http::Method::POST,
                kids_lib::types::ApiPath::from_segments(["personal-sessions"]),
                Some(serde_json::json!({
                    "actor_user_id": admin_id,
                    "expires_in": validity_seconds,
                    "scope": scope,
                    "human_name": format!("KIDS: {scope}"),
                })),
            )
            .await?
            .data)
    }

    async fn regenerate_personal_session(
        &self,
        id: &dto::mas::internal::personal_session::Id,
    ) -> Result<dto::mas::PersonalSessionResponse, kids_lib::error::KidsError> {
        let validity = chrono::Duration::seconds(self.config.api_access.token_validity_seconds as i64);
        tracing::debug!(
            personal_session_id = tracing::field::display(id),
            validity = validity.to_string(),
            "Regenerating a personal session"
        );
        let validity_seconds = validity.num_seconds();
        Ok(self
            .send_mas_admin_request_single(
                http::Method::POST,
                kids_lib::types::ApiPath::from_segments(["personal-sessions", id.as_str(), "regenerate"]),
                Some(serde_json::json!({
                    "expires_in": validity_seconds,
                })),
            )
            .await?
            .data)
    }

    async fn get_admin_access_token(
        &self,
        scope: crate::target::types::AccessTokenScope,
    ) -> Result<crate::target::types::AccessToken, kids_lib::error::KidsError> {
        let mut access_tokens = self.api_access.access_tokens.lock().await;
        if let Some(access_token) = access_tokens.get_mut(&scope) {
            if access_token
                .expires_at
                .is_none_or(|expires_at| expires_at > chrono::Utc::now() + chrono::TimeDelta::seconds(5))
            {
                tracing::trace!(scope = scope.0, "Access token is still valid in five seconds time, doing nothing");
                return Ok(access_token.clone());
            } else {
                tracing::trace!(scope = scope.0, "Access token will be regenerated");
                let personal_session = self.regenerate_personal_session(&access_token.id).await?;
                access_token.access_token = crate::target::types::AccessTokenToken(personal_session.attributes.access_token);
                access_token.expires_at = personal_session.attributes.expires_at;
                return Ok(access_token.clone());
            }
        }
        tracing::info!(scope = scope.0, "Generating first token for this scope");
        let pat = self.new_admin_personal_session(&scope).await?;
        let access_token = crate::target::types::AccessToken {
            id: pat.id,
            access_token: crate::target::types::AccessTokenToken(pat.attributes.access_token),
            expires_at: pat.attributes.expires_at,
            scope: crate::target::types::AccessTokenScope(pat.attributes.scope),
        };
        access_tokens.insert(scope.clone(), access_token.clone());
        Ok(access_token)
    }

    fn construct_unauthenticated_request<B: serde::Serialize>(&self, method: http::Method, url: String, body: Option<B>) -> RequestBuilder {
        let mut builder = self.http_client.request(method, &url);
        if let Some(body) = body {
            builder = builder.json(&body)
        }
        builder
    }

    async fn construct_authenticated_request<B: serde::Serialize>(
        &self,
        method: http::Method,
        url: String,
        body: Option<B>,
        token: impl std::fmt::Display,
    ) -> RequestBuilder {
        let mut builder = self.construct_unauthenticated_request(method, url, body);
        builder = builder.bearer_auth(token);
        builder
    }

    async fn send_request<T: serde::de::DeserializeOwned>(&self, request: RequestBuilder) -> Result<T, KidsError> {
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let url = response.url().to_string();
                if status.is_success() {
                    if status == http::StatusCode::NO_CONTENT {
                        // Try to decode an empty object.
                        // Even for a struct with no members, deserializing it from the empty string (the real content of the response)
                        // is not possible. In case this fails, we cannot rescue the situation as our caller
                        // expects a `T` which we cannot construct here.
                        return serde_json::from_str("{}")
                            .map_err(|error| KidsError::ApiOperationFailed(kids_lib::error::no_context(), status.as_u16(), url, anyhow!(error)));
                    }
                    return match response.json().await {
                        Ok(json) => Ok(json),
                        Err(error) => Err(KidsError::ApiOperationFailed(
                            kids_lib::error::no_context(),
                            status.as_u16(),
                            url,
                            anyhow!(error),
                        )),
                    };
                }

                let error_information = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to obtain error information from response text".to_string());

                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Err(KidsError::AuthenticationFailed(
                        kids_lib::error::no_context(),
                        status.as_u16(),
                        url,
                        anyhow!(error_information),
                    ));
                }

                Err(KidsError::ApiOperationFailed(
                    kids_lib::error::no_context(),
                    status.as_u16(),
                    url,
                    anyhow!(error_information),
                ))
            }
            Err(e) => Err(KidsError::RequestFailed(kids_lib::error::no_context(), anyhow!(e))),
        }
    }

    async fn send_mas_admin_request_single<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<dto::mas::SingleResponse<T>, KidsError> {
        self.send_mas_admin_request(method, path, body).await
    }

    async fn send_mas_admin_request_single_get<T: serde::de::DeserializeOwned>(
        &self,
        path: kids_lib::types::ApiPath,
    ) -> Result<dto::mas::SingleResponse<T>, KidsError> {
        self.send_mas_admin_request_single::<(), _>(http::Method::GET, path, None).await
    }

    async fn send_mas_admin_request_list<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: http::Method,
        mut path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<dto::mas::ListResponse<T>, KidsError> {
        path.add_query_parameter("count", "true");
        path.add_query_parameter("page[first]", "100000");
        let result: dto::mas::ListResponse<T> = self.send_mas_admin_request(method, path.clone(), body).await?;
        let reported_total_count = result.meta.count;
        let observed_count = result.data.len() as u64;
        if reported_total_count != observed_count {
            tracing::error!(
                reported_total_count,
                observed_count,
                "Number of returned entities does not match reported total number of entities"
            );
            return Err(kids_lib::error::KidsError::RequestFailed(
                format!("{path}"),
                anyhow::anyhow!("Number of returned entities does not match reported total number of entities"),
            ));
        }
        Ok(result)
    }

    async fn send_mas_admin_request_list_get<T: serde::de::DeserializeOwned>(
        &self,
        path: kids_lib::types::ApiPath,
    ) -> Result<dto::mas::ListResponse<T>, KidsError> {
        self.send_mas_admin_request_list::<(), _>(http::Method::GET, path, None).await
    }

    async fn send_mas_admin_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<T, KidsError> {
        let mas_access_token = self.api_access.mas_account.get_access_token().await.map_err(error_to_kids_error)?;
        let request = self
            .construct_authenticated_request(method, format!("{}api/admin/v1/{}", self.parsed_mas_url, path), body, mas_access_token)
            .await;
        self.send_request(request).await
    }

    async fn send_client_api_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<T, KidsError> {
        let token = self.get_admin_access_token(crate::target::types::matrix_api_scope()).await?;
        let request = self
            .construct_authenticated_request(
                method,
                format!("{}_matrix/client/v3/{}", self.parsed_homeserver_url, path),
                body,
                &token.access_token.0,
            )
            .await;
        self.send_request(request).await
    }

    async fn client_api_get<T: serde::de::DeserializeOwned>(&self, path: kids_lib::types::ApiPath) -> Result<T, KidsError> {
        self.send_client_api_request::<(), T>(http::Method::GET, path, None).await
    }

    async fn send_admin_api_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        api_version: &str,
        method: http::Method,
        path: kids_lib::types::ApiPath,
        body: Option<B>,
    ) -> Result<T, KidsError> {
        let token = self.get_admin_access_token(crate::target::types::synapse_api_scope()).await?;
        let request = self
            .construct_authenticated_request(
                method,
                format!("{}_synapse/admin/{}/{}", self.parsed_homeserver_url, api_version, path),
                body,
                &token.access_token.0,
            )
            .await;
        self.send_request(request).await
    }

    async fn admin_api_get<T: serde::de::DeserializeOwned>(&self, api_version: &str, path: kids_lib::types::ApiPath) -> Result<T, KidsError> {
        self.send_admin_api_request::<(), T>(api_version, http::Method::GET, path, None).await
    }

    fn static_room_power_level_content_override(&self) -> serde_json::Value {
        serde_json::json!({
            "users": {
                self.config.matrix_syncer_user_id.display(): 100,
            },
            "events": {
                "m.room.avatar": 0,
                "m.room.topic": 0,
                "m.room.name": 50,
                "m.room.power_levels": 100,
                "m.room.history_visibility": 100,
                "m.room.canonical_alias": 50,
                "m.room.tombstone": 100,
                "m.room.server_acl": 100,
                "m.room.encryption": 100,
                "im.vector.modular.widgets": 0,
            },
            "notifications": {
                "room": 0,
            },
            "users_default": 0,
            "events_default": 0,
            "state_default": 50,
            "ban": 50,
            "kick": 50,
            "redact": 50,
            "invite": 50,
            "historical": 100,
        })
    }

    fn static_room_initial_state(&self) -> Vec<serde_json::Value> {
        let room_encryption_json = serde_json::json!({
            "type": "m.room.encryption",
            "content": {
                "algorithm": "m.megolm.v1.aes-sha2",
            }
        });

        let guest_access_json = serde_json::json!({
            "type": "m.room.guest_access",
            "content": {
                "guest_access": "forbidden",
            }
        });

        vec![room_encryption_json, guest_access_json]
    }

    fn room_alias_local_part(&self, group_path: &str) -> String {
        // "The localpart of a room alias may contain any valid non-surrogate Unicode codepoints except : and NUL."
        // See https://spec.matrix.org/v1.15/appendices/#room-aliases
        let sanitized_path = group_path
            .chars()
            // Rust characters cannot be surrogate Unicode codepoints.
            .filter(|c| *c != ':' && *c != '\0')
            .collect::<String>()
            .to_lowercase()
            .trim_matches('/')
            .replace("/", "-")
            // Replace whitespace characters as they are not supported by Synapse.
            // See https://github.com/element-hq/synapse/issues/20261 for more info.
            .replace(
                |ch| {
                    // These are the same characters as used by Synapse as of
                    // Python 3.14.7 (https://docs.python.org/3.14/library/string.html#string.whitespace)
                    // and Synapse 1.161.0 (https://github.com/element-hq/synapse/issues/20261).
                    const WHITESPACE_CHARACTERS: [char; 6] = [' ', '\t', '\n', '\r', '\x0b', '\x0c'];
                    WHITESPACE_CHARACTERS.contains(&ch)
                },
                "_",
            );

        // The complete alias must not exceed 255 characters including the leading '#'
        // and the ':' delimiter between local part and domain.
        // If the generated alias is longer, we use the last characters from our sanitized path
        // so in a deep group hierarchy with long paths, room aliases are still distinct for rooms
        // derived from sibling groups.
        let mut path_start_index = 0;
        let maximum_allowed_path_length = 255 - 1 - 1 - self.homeserver_domain().display().len();
        if sanitized_path.len() > maximum_allowed_path_length {
            path_start_index = sanitized_path.len() - maximum_allowed_path_length
        }
        sanitized_path[path_start_index..].to_owned()
    }
}

#[async_trait::async_trait]
impl SynapseApi for SynapseClient {
    fn user_is_matrix_syncer(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> bool {
        *matrix_user_id == self.config.matrix_syncer_user_id
    }

    fn homeserver_domain(&self) -> &crate::target::types::MatrixServerName {
        &self.config.matrix_syncer_user_id.homeserver
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3joined_rooms
    async fn get_joined_rooms_of_syncer(&self) -> Result<dto::matrix::JoinedRoomsResponse, KidsError> {
        self.client_api_get(kids_lib::types::ApiPath::from_segments(["joined_rooms"])).await
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#post_matrixclientv3roomsroomidleave
    async fn syncer_leave_room(&self, matrix_room_id: &str) -> Result<(), KidsError> {
        let _ = self
            .send_client_api_request::<(), dto::IgnoredResponse>(
                http::Method::POST,
                kids_lib::types::ApiPath::from_segments(["rooms", matrix_room_id, "leave"]),
                None,
            )
            .await?;
        Ok(())
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#list-accounts-v3
    async fn get_mas_users(&self) -> Result<Vec<dto::mas::UserResponse>, KidsError> {
        let mas_users: Vec<dto::mas::UserResponse> = self
            .send_mas_admin_request_list_get(kids_lib::types::ApiPath::from_segments(["users"]))
            .await?
            .data;
        Ok(mas_users)
    }

    async fn get_user_emails(&self, mas_user_id: &dto::mas::internal::user::Id) -> Result<Vec<String>, KidsError> {
        let emails: Vec<dto::mas::UserEmailResponse> = self
            .send_mas_admin_request_list_get(kids_lib::types::ApiPath::from_segments_and_query(
                ["user-emails"],
                [("filter[user]", mas_user_id.as_str())],
            ))
            .await?
            .data;
        Ok(emails.into_iter().map(|data| data.attributes.email).collect())
    }

    async fn set_user_emails(&self, mas_user_id: &dto::mas::internal::user::Id, emails: &[String]) -> Result<(), KidsError> {
        let user_emails: Vec<dto::mas::UserEmailResponse> = self
            .send_mas_admin_request_list_get(kids_lib::types::ApiPath::from_segments_and_query(
                ["user-emails"],
                [("filter[user]", mas_user_id.as_str())],
            ))
            .await?
            .data;
        let user_emails_to_remove = user_emails.iter().filter(|data| !emails.contains(&data.attributes.email));
        for user_email in user_emails_to_remove {
            self.send_mas_admin_request::<(), dto::IgnoredResponse>(
                http::Method::DELETE,
                kids_lib::types::ApiPath::from_segments(["user-emails", user_email.id.as_str()]),
                None,
            )
            .await?;
        }
        let emails_to_add = emails.iter().filter(|email| !user_emails.iter().any(|data| data.attributes.email == **email));
        for email in emails_to_add {
            self.send_mas_admin_request::<_, dto::IgnoredResponse>(
                http::Method::POST,
                kids_lib::types::ApiPath::from_segments(["user-emails"]),
                Some(serde_json::json!({
                    "user_id": mas_user_id,
                    "email": email,
                })),
            )
            .await?;
        }
        Ok(())
    }

    /// See https://element-hq.github.io/matrix-authentication-service/api/index.html#/user/lockUser
    async fn lock_user(&self, mas_user_id: &dto::mas::internal::user::Id) -> Result<(), KidsError> {
        self.send_mas_admin_request_single::<_, dto::IgnoredResponse>(
            http::Method::POST,
            kids_lib::types::ApiPath::from_segments(["users", mas_user_id.as_str(), "lock"]),
            Some(serde_json::json!({
                "skip_erase": false
            })),
        )
        .await?;
        Ok(())
    }

    /// See https://element-hq.github.io/matrix-authentication-service/api/index.html#/user/unlockUser
    async fn unlock_user(&self, mas_user_id: &dto::mas::internal::user::Id) -> Result<(), KidsError> {
        self.send_mas_admin_request_single::<_, dto::IgnoredResponse>(
            http::Method::POST,
            kids_lib::types::ApiPath::from_segments(["users", mas_user_id.as_str(), "unlock"]),
            Some(serde_json::json!({
                "skip_erase": false
            })),
        )
        .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3profileuseriddisplayname
    async fn set_user_display_name(&self, matrix_user_id: &crate::target::types::MatrixUserId, display_name: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                kids_lib::types::ApiPath::from_segments(["profile", &matrix_user_id.display(), "displayname"]),
                Some(serde_json::json!({"displayname": display_name})),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3profileuseriddisplayname
    async fn get_user_display_name(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<Option<String>, KidsError> {
        let response: dto::matrix::UserDisplayNameResponse = self
            .client_api_get(kids_lib::types::ApiPath::from_segments(["profile", &matrix_user_id.display(), "displayname"]))
            .await?;
        Ok(response.display_name)
    }

    /// See https://element-hq.github.io/matrix-authentication-service/api/index.html#/user/userSetAdmin
    async fn set_admin_status(&self, mas_user_id: &dto::mas::internal::user::Id, should_be_admin: bool) -> Result<(), KidsError> {
        self.send_mas_admin_request_single::<_, dto::IgnoredResponse>(
            http::Method::POST,
            kids_lib::types::ApiPath::from_segments(["users", mas_user_id.as_str(), "set-admin"]),
            Some(serde_json::json!({
                "admin": should_be_admin,
            })),
        )
        .await?;
        Ok(())
    }

    async fn create_user(
        &self,
        matrix_user_id: &crate::target::types::MatrixUserId,
        source_user_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<dto::mas::UserResponse, KidsError> {
        let create_user_result = self
            .send_mas_admin_request_single::<_, dto::mas::UserResponse>(
                http::Method::POST,
                kids_lib::types::ApiPath::from_segments(["users"]),
                Some(serde_json::json!({
                    "username": matrix_user_id.username,
                })),
            )
            .await;
        let created_user_response = match create_user_result {
            Ok(response) => response.data,
            Err(err) => {
                tracing::warn!(
                    matrix_user_id = matrix_user_id.display(),
                    source_user_id,
                    "Error creating user. In case of a 409, this might be caused by a previously aborted execution where a user account without a link to the source user was created. In this case, manually link the account and re-run the syncer."
                );
                return Err(err);
            }
        };
        self.associate_source_user_id_to_user(matrix_user_id, source_user_id).await?;
        Ok(created_user_response)
    }

    async fn mas_user_for_matrix_user_id(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<dto::mas::UserResponse, KidsError> {
        Ok(self
            .send_mas_admin_request_single_get::<dto::mas::UserResponse>(kids_lib::types::ApiPath::from_segments([
                "users",
                "by-username",
                &matrix_user_id.username,
            ]))
            .await?
            .data)
    }

    async fn associate_source_user_id_to_user(
        &self,
        matrix_user_id: &crate::target::types::MatrixUserId,
        source_user_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<(), KidsError> {
        let mas_user_id = self.mas_user_for_matrix_user_id(matrix_user_id).await?.id;
        self.send_mas_admin_request::<_, dto::IgnoredResponse>(
            http::Method::POST,
            kids_lib::types::ApiPath::from_segments(["upstream-oauth-links"]),
            Some(serde_json::json!({
                "user_id": mas_user_id,
                "provider_id": self.config.matrix_source_oidc_provider_ulid,
                "subject": source_user_id
            })),
        )
        .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#post_matrixclientv3createroom
    async fn create_room(&self, name: &str, path: &str) -> Result<dto::matrix::RoomCreationResponse, KidsError> {
        self.send_client_api_request(
            http::Method::POST,
            kids_lib::types::ApiPath::from_segments(["createRoom"]),
            Some(serde_json::json!({
                "name": name,
                "visibility": "private",
                "preset": "private_chat",
                "initial_state": self.static_room_initial_state(),
                "power_level_content_override": self.static_room_power_level_content_override(),
                "room_alias_name": self.room_alias_local_part(path),
                // Fix room version to 11.
                // Version 12 changed some things about room creation.
                // See https://rechenknecht.net/giz/matrix/keycloak-matrix-syncer/-/merge_requests/5
                // for reference where we did the same in a similar project.
                // See https://matrix.org/blog/2025/07/security-predisclosure/
                // and https://faq.tickets.tu-dresden.de/otrs/public.pl?Action=PublicFAQZoom;ItemID=1304
                // for more info on the migration to v12.
                // Make sure to thoroughly test the effects of updating this value
                // on all aspects of the syncer!
                "room_version": "11"
            })),
        )
        .await
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/rooms.html#version-2-new-version
    async fn delete_room(&self, matrix_room_id: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v1", // We are intentionally using the older, blocking version of the API here.
                http::Method::DELETE,
                kids_lib::types::ApiPath::from_segments(["rooms", matrix_room_id]),
                Some(&serde_json::json!({
                    "purge": true // Deletes all traces of the room from the database.
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3roomsroomidstateeventtypestatekey
    /// We are using a custom state event type here, and no stateKey
    /// (note that having an empty stateKey is not unusual, but actually the default)
    async fn associate_source_group_id_to_room(
        &self,
        matrix_room_id: &str,
        source_group_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                kids_lib::types::ApiPath::from_segments(["rooms", matrix_room_id, "state", SYNCER_ROOM_METADATA_EVENT]),
                Some(serde_json::json!({
                    "source_id": source_group_id
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3roomsroomideventeventid
    /// We are using a custom state event type here, which must match the one we created via
    /// [SynapseClient::associate_source_group_id_to_room].
    async fn get_room_associated_source_group_id(&self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError> {
        let event: dto::matrix::RoomGlobalIdEvent = self
            .client_api_get(kids_lib::types::ApiPath::from_segments([
                "rooms",
                matrix_room_id,
                "state",
                SYNCER_ROOM_METADATA_EVENT,
            ]))
            .await?;
        tracing::debug!(source_id = event.source_id, matrix_room_id, "Found mapping");
        Ok(event.source_id)
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3useruseridroomsroomidaccount_datatype
    /// Old version of storing syncer metadata for a room in the account data of the sync user
    /// instead of in the metadata of a room directly.
    async fn get_room_associated_source_group_id_v1(&self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError> {
        let account_data_event: serde_json::Value = self
            .client_api_get(kids_lib::types::ApiPath::from_segments([
                "user",
                &self.config.matrix_syncer_user_id.display(),
                "rooms",
                matrix_room_id,
                "account_data",
                &format!("{}.room_sync", self.config.matrix_namespace),
            ]))
            .await?;
        match account_data_event.get(format!("{}.room_sync.source_id", self.config.matrix_namespace)) {
            Some(val) => Ok(val.as_str().unwrap().to_string()),
            None => Err(KidsError::InternalError(anyhow::anyhow!(
                "Old version of room sync event did not contain expected attribute"
            ))),
        }
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3roomsroomidstateeventtypestatekey
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomname
    async fn set_room_display_name(&self, matrix_room_id: &str, display_name: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                kids_lib::types::ApiPath::from_segments(["rooms", matrix_room_id, "state", "m.room.name"]),
                Some(&dto::matrix::RoomNameEvent {
                    name: display_name.to_string(),
                }),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3roomsroomideventeventid
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomname
    async fn get_room_display_name(&self, matrix_room_id: &str) -> Result<String, KidsError> {
        let room_name_event: dto::matrix::RoomNameEvent = self
            .client_api_get(kids_lib::types::ApiPath::from_segments(["rooms", matrix_room_id, "state", "m.room.name"]))
            .await?;
        Ok(room_name_event.name)
    }

    fn full_room_alias(&self, group_path: &str) -> String {
        "#".to_owned() + &self.room_alias_local_part(group_path) + ":" + &self.homeserver_domain().display()
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3directoryroomroomalias.
    async fn create_room_alias(&self, matrix_room_id: &str, alias: &str) -> Result<(), KidsError> {
        let res: Result<dto::IgnoredResponse, KidsError> = self
            .send_client_api_request(
                http::Method::PUT,
                kids_lib::types::ApiPath::from_segments(["directory", "room", alias]),
                Some(&serde_json::json!({
                    "room_id": matrix_room_id
                })),
            )
            .await;

        if let Err(KidsError::ApiOperationFailed(_, 409, ..)) = res {
            tracing::warn!(matrix_room_id, alias, "Room alias already exists");
            return Ok(());
        }

        match res {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#delete_matrixclientv3directoryroomroomalias
    async fn delete_room_alias(&self, alias: &str) -> Result<(), KidsError> {
        let _ = self
            .send_client_api_request::<(), serde_json::Value>(
                http::Method::DELETE,
                kids_lib::types::ApiPath::from_segments(["directory", "room", alias]),
                None,
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3roomsroomidstateeventtypestatekey
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomcanonical_alias
    async fn set_room_canonical_alias(&self, matrix_room_id: &str, canonical_alias: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                kids_lib::types::ApiPath::from_segments(["rooms", matrix_room_id, "state", "m.room.canonical_alias"]),
                Some(dto::matrix::RoomCanonicalAliasEvent {
                    alias: canonical_alias.to_owned(),
                    alt_aliases: None,
                }),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3roomsroomideventeventid
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomcanonical_alias
    async fn get_room_canonical_alias(&self, room_id: &str) -> Result<dto::matrix::RoomCanonicalAliasEvent, KidsError> {
        self.client_api_get(kids_lib::types::ApiPath::from_segments(["rooms", room_id, "state", "m.room.canonical_alias"]))
            .await
    }

    /// See https://element-hq.github.io/matrix-authentication-service/api/index.html#/upstream-oauth-link/listUpstreamOAuthLinks.
    async fn get_source_user_id_for_mas_user_id(
        &self,
        mas_user_id: &dto::mas::internal::user::Id,
    ) -> Result<Option<kids_lib::types::SharedResourceIdentifier>, KidsError> {
        let response: Vec<dto::mas::UpstreamOauthLinkResponse> = self
            .send_mas_admin_request_list_get(kids_lib::types::ApiPath::from_segments_and_query(
                ["upstream-oauth-links"],
                [
                    ("filter[user]", mas_user_id.as_str()),
                    ("filter[provider]", self.config.matrix_source_oidc_provider_ulid.as_str()),
                ],
            ))
            .await?
            .data;
        match response.len() {
            0 => Ok(None),
            1 => Ok(Some(response.into_iter().next().expect("We have just checked the length").attributes.subject)),
            2.. => Err(KidsError::InternalError(anyhow::anyhow!(
                "Did find multiple external ID for source auth provider for MAS user: {mas_user_id}"
            ))),
        }
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#list-joined-rooms-of-a-user
    async fn get_user_joined_rooms(&self, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<dto::matrix::UserJoinedRoomsResponse, KidsError> {
        self.admin_api_get(
            "v1",
            kids_lib::types::ApiPath::from_segments(["users", &matrix_user_id.display(), "joined_rooms"]),
        )
        .await
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3roomsroomidjoined_members.
    async fn get_room_joined_users(&self, matrix_room_id: &str) -> Result<dto::matrix::RoomJoinedUsersResponse, KidsError> {
        self.client_api_get(kids_lib::types::ApiPath::from_segments(["rooms", matrix_room_id, "joined_members"]))
            .await
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/room_membership.html.
    async fn join_user_to_room(&self, matrix_group_id: &str, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v1",
                http::Method::POST,
                kids_lib::types::ApiPath::from_segments(["join", matrix_group_id]),
                Some(&serde_json::json!({
                    "user_id": matrix_user_id
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#post_matrixclientv3roomsroomidkick.
    async fn kick_user_from_room(&self, matrix_group_id: &str, matrix_user_id: &crate::target::types::MatrixUserId) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::POST,
                kids_lib::types::ApiPath::from_segments(["rooms", matrix_group_id, "kick"]),
                Some(serde_json::json!({
                    "user_id": matrix_user_id
                })),
            )
            .await?;
        Ok(())
    }
}
