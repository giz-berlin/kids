use anyhow::anyhow;
use reqwest::RequestBuilder;

use kids_lib::error::KidsError;

use crate::target::dto;

#[derive(serde::Deserialize, Clone)]
pub struct SynapseApiConfig {
    /// URL of the Matrix homeserver (probably similar to https://matrix.example.com).
    pub matrix_homeserver_url: String,
    /// The ID of the source (for example, Keycloak) as identity provider in the Synapse config.
    pub matrix_source_oidc_provider_id: String,
    /// User ID of a Matrix user that must have administrative access.
    /// This user will perform all operations required for syncing users and rooms.
    /// Create a dedicated user if you can.
    /// (probably similar to `@keycloak-sync:matrix.example.org`)
    pub matrix_syncer_user_id: String,
    /// Password of the syncer Matrix user.
    pub matrix_syncer_password: String,
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
    fn user_is_matrix_syncer(&self, matrix_user_id: &str) -> bool;
    fn homeserver_domain(&self) -> &str;
    async fn get_joined_rooms_of_syncer(&mut self) -> Result<dto::JoinedRoomsResponse, KidsError>;
    async fn syncer_leave_room(&mut self, matrix_room_id: &str) -> Result<(), KidsError>;
    async fn get_users(&mut self) -> Result<dto::AllUsersResponse, KidsError>;
    async fn deactivate_user(&mut self, matrix_user_id: &str) -> Result<(), KidsError>;
    async fn get_user_three_pids(&mut self, matrix_user_id: &str) -> Result<Vec<dto::ThreePID>, KidsError>;
    async fn set_user_three_pids(&mut self, matrix_user_id: &str, three_pids: &[dto::ThreePID]) -> Result<(), KidsError>;
    async fn lock_user(&mut self, matrix_user_id: &str) -> Result<(), KidsError>;
    async fn unlock_user(&mut self, matrix_user_id: &str) -> Result<(), KidsError>;
    async fn set_user_display_name(&mut self, matrix_user_id: &str, display_name: &str) -> Result<(), KidsError>;
    async fn get_user_display_name(&mut self, matrix_user_id: &str) -> Result<Option<String>, KidsError>;
    async fn create_user(&mut self, matrix_user_id: &str, source_user_id: &str) -> Result<dto::User, KidsError>;

    async fn create_room(&mut self, name: &str, path: &str) -> Result<dto::RoomCreationResponse, KidsError>;
    async fn delete_room(&mut self, matrix_room_id: &str) -> Result<(), KidsError>;
    async fn associate_source_group_id_to_room(
        &mut self,
        matrix_room_id: &str,
        source_group_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<(), KidsError>;
    async fn get_room_associated_source_group_id(&mut self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError>;
    async fn get_room_associated_source_group_id_v1(&mut self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError>;
    async fn set_room_display_name(&mut self, matrix_room_id: &str, display_name: &str) -> Result<(), KidsError>;
    async fn get_room_display_name(&mut self, matrix_room_id: &str) -> Result<String, KidsError>;

    fn full_room_alias(&self, group_path: &str) -> String;
    async fn create_room_alias(&mut self, matrix_room_id: &str, alias: &str) -> Result<(), KidsError>;
    async fn delete_room_alias(&mut self, alias: &str) -> Result<(), KidsError>;
    async fn set_room_canonical_alias(&mut self, matrix_room_id: &str, canonical_alias: &str) -> Result<(), KidsError>;
    async fn get_room_canonical_alias(&mut self, matrix_room_id: &str) -> Result<dto::RoomCanonicalAliasEvent, KidsError>;

    async fn get_source_user_id_for_matrix_user_id(&mut self, matrix_user_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError>;
    async fn get_user_joined_rooms(&mut self, matrix_user_id: &str) -> Result<dto::UserJoinedRoomsResponse, KidsError>;
    async fn get_room_joined_users(&mut self, matrix_room_id: &str) -> Result<dto::RoomJoinedUsersResponse, KidsError>;
    async fn join_user_to_room(&mut self, matrix_room_id: &str, matrix_user_id: &str) -> Result<(), KidsError>;
    async fn kick_user_from_room(&mut self, matrix_room_id: &str, matrix_user_id: &str) -> Result<(), KidsError>;
}

pub struct SynapseClient {
    authentication: Authentication,
    config: SynapseApiConfig,
    http_client: reqwest::Client,
    parsed_homeserver_url: url::Url,
}

struct Authentication {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Page size requested when loading users.
/// Because we don't support pagination, this needs to be large enough to return all users
/// known to Synapse.
const ALL_USERS: u32 = u32::MAX;
/// The name of the room state event the syncer stores its metadata in
/// (such as the mapping of room to source group).
const SYNCER_ROOM_METADATA_EVENT: &str = "m.room.kids.room_sync";

/// A properly encoded URL path.
///
/// Use [`from_segments`](Self::from_segments) or
/// [`from_segments_and_query``](Self::from_segments_and_query) to create it.
#[derive(Debug)]
struct ApiPath(String);

impl ApiPath {
    /// First, [url-encodes](urlencoding::encode) each `segment` in `segments`.
    ///
    /// Then, returns a path of the form `segments[0]/segments[1]/.../segments[N-1]` for the encoded `segments`.
    fn from_segments<const N: usize>(segments: [&str; N]) -> Self {
        let segments = segments.into_iter().map(|segment| urlencoding::encode(segment)).collect::<Vec<_>>();
        Self(segments.join("/"))
    }
    /// First, [url-encodes](urlencoding::encode) each `segment` in `segments` and each `value` in `query_parameters[i].1`.
    ///
    /// Then, returns a path of the form `segments[0]/.../segments[N-1]?query_parameters[0].0=query_parameters[0].1&...&query_parameters[M-1].0=query_parameters[M-1].1`
    /// for the encoded `segments` and `query_parameters[i].1`.
    ///
    /// It does not encode the keys of the query parameters.
    fn from_segments_and_query<const N: usize, const M: usize>(segments: [&str; N], query_parameters: [(&str, &str); M]) -> Self {
        let segments = segments.into_iter().map(|segment| urlencoding::encode(segment)).collect::<Vec<_>>();
        let path_section = segments.join("/");
        let query_parameters = query_parameters
            .into_iter()
            .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
            .collect::<Vec<_>>();
        let query_section = query_parameters.join("&");
        Self(format!("{path_section}?{query_section}"))
    }
}
impl std::fmt::Display for ApiPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl SynapseClient {
    pub async fn new(config: SynapseApiConfig) -> Result<Self, KidsError> {
        let parsed_homeserver_url = url::Url::parse(&config.matrix_homeserver_url).expect("Homeserver URL should be parseable");
        tracing::info!(homeserver_url=%parsed_homeserver_url, "Connecting to homeserver");

        let mut builder = reqwest::Client::builder();
        if config.insecure_disable_tls_verification {
            tracing::warn!("Verification of Matrix server certificate is disabled. Do not use this setting in a production environment!");
            builder = builder.danger_accept_invalid_certs(true);
        }
        let client = builder.build().unwrap();

        let mut synapse_client = SynapseClient {
            config,
            http_client: client,
            authentication: Authentication {
                access_token: None,
                refresh_token: None,
                expires_at: None,
            },
            parsed_homeserver_url,
        };

        synapse_client.login().await?;

        Ok(synapse_client)
    }

    async fn login(&mut self) -> Result<(), KidsError> {
        let token_response: dto::MatrixAuthentication = self
            .send_client_api_request_unauthenticated(
                http::Method::POST,
                ApiPath::from_segments(["login"]),
                Some(serde_json::json!({
                    "type": "m.login.password",
                    "identifier": {
                        "type": "m.id.user",
                        "user": &self.config.matrix_syncer_user_id,
                      },
                    "password": &self.config.matrix_syncer_password,
                    "refresh_token": true
                })),
            )
            .await?;
        self.authentication.access_token = Some(token_response.access_token);
        self.authentication.refresh_token = token_response.refresh_token;
        if let Some(expires_in_ms) = token_response.expires_in_ms {
            self.authentication.expires_at = Some(chrono::Utc::now() + chrono::Duration::milliseconds(expires_in_ms));
        } else {
            self.authentication.expires_at = None
        }
        tracing::info!(homeserver_url=%self.parsed_homeserver_url, "Logged in to homeserver");
        Ok(())
    }

    async fn refresh_access_token_if_necessary(&mut self) -> Result<(), KidsError> {
        // In order to avoid the access token expiring between this check and the actual request,
        // we also refresh tokens that are not yet expired but will be soon.
        if let Some(expires_at) = self.authentication.expires_at
            && expires_at - chrono::Duration::seconds(5) < chrono::Utc::now()
        {
            tracing::debug!("Refreshing access token");
            if self.authentication.refresh_token.is_none() {
                return self.login().await;
            }
            let token_response_res: Result<dto::MatrixAuthentication, KidsError> = self
                .send_client_api_request_unauthenticated(
                    http::Method::POST,
                    ApiPath::from_segments(["refresh"]),
                    Some(serde_json::json!({
                        "refresh_token": self.authentication.refresh_token,
                    })),
                )
                .await;

            // Refresh tokens might expire (although they are by default valid for infinite lifetime:
            // https://element-hq.github.io/synapse/v1.159/usage/configuration/user_authentication/refresh_tokens.html)
            match token_response_res {
                Ok(token_response) => {
                    self.authentication.access_token = Some(token_response.access_token);
                    if let Some(refresh_token) = token_response.refresh_token {
                        self.authentication.refresh_token = Some(refresh_token);
                    }
                    if let Some(expires_in_ms) = token_response.expires_in_ms {
                        self.authentication.expires_at = Some(chrono::Utc::now() + chrono::Duration::milliseconds(expires_in_ms));
                    } else {
                        self.authentication.expires_at = None
                    }
                }
                Err(error) => {
                    tracing::warn!(error=%error, "Unable to refresh access token by refresh token, trying again with re-login");
                    self.login().await?;
                }
            }
        }

        Ok(())
    }

    fn construct_unauthenticated_request<B: serde::Serialize>(&mut self, method: http::Method, url: String, body: Option<B>) -> RequestBuilder {
        let mut builder = self.http_client.request(method, &url);
        if let Some(body) = body {
            builder = builder.json(&body)
        }
        builder
    }

    async fn construct_authenticated_request<B: serde::Serialize>(
        &mut self,
        method: http::Method,
        url: String,
        body: Option<B>,
    ) -> Result<RequestBuilder, KidsError> {
        let mut builder = self.construct_unauthenticated_request(method, url, body);
        self.refresh_access_token_if_necessary().await?;
        // Since we just refreshed the access token above, we can safely access it here.
        builder = builder.bearer_auth(self.authentication.access_token.clone().unwrap());
        Ok(builder)
    }

    async fn send_request<T: serde::de::DeserializeOwned>(&mut self, request: RequestBuilder) -> Result<T, KidsError> {
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let url = response.url().to_string();
                if status.is_success() {
                    return match response.json().await {
                        Ok(json) => Ok(json),
                        Err(error) => Err(KidsError::ApiOperationFailed(
                            kids_lib::error::NO_CONTEXT.to_string(),
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
                        kids_lib::error::NO_CONTEXT.to_string(),
                        status.as_u16(),
                        url,
                        anyhow!(error_information),
                    ));
                }

                Err(KidsError::ApiOperationFailed(
                    kids_lib::error::NO_CONTEXT.to_string(),
                    status.as_u16(),
                    url,
                    anyhow!(error_information),
                ))
            }
            Err(e) => Err(KidsError::RequestFailed(kids_lib::error::NO_CONTEXT.to_string(), anyhow!(e))),
        }
    }

    async fn send_client_api_request_unauthenticated<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        method: http::Method,
        path: ApiPath,
        body: Option<B>,
    ) -> Result<T, KidsError> {
        let request = self.construct_unauthenticated_request(method, format!("{}_matrix/client/v3/{}", self.parsed_homeserver_url, path), body);
        self.send_request(request).await
    }

    async fn send_client_api_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        method: http::Method,
        path: ApiPath,
        body: Option<B>,
    ) -> Result<T, KidsError> {
        let request = self
            .construct_authenticated_request(method, format!("{}_matrix/client/v3/{}", self.parsed_homeserver_url, path), body)
            .await?;
        self.send_request(request).await
    }

    async fn client_api_get<T: serde::de::DeserializeOwned>(&mut self, path: ApiPath) -> Result<T, KidsError> {
        self.send_client_api_request::<(), T>(http::Method::GET, path, None).await
    }

    async fn send_admin_api_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        api_version: &str,
        method: http::Method,
        path: ApiPath,
        body: Option<B>,
    ) -> Result<T, KidsError> {
        let request = self
            .construct_authenticated_request(method, format!("{}_synapse/admin/{}/{}", self.parsed_homeserver_url, api_version, path), body)
            .await?;
        self.send_request(request).await
    }

    async fn admin_api_get<T: serde::de::DeserializeOwned>(&mut self, api_version: &str, path: ApiPath) -> Result<T, KidsError> {
        self.send_admin_api_request::<(), T>(api_version, http::Method::GET, path, None).await
    }

    fn static_room_power_level_content_override(&self) -> serde_json::Value {
        serde_json::json!({
            "users": {
                self.config.matrix_syncer_user_id.to_owned(): 100,
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
            .replace("/", "-");

        // The complete alias must not exceed 255 characters including the leading '#'
        // and the ':' delimiter between local part and domain.
        // If the generated alias is longer, we use the last characters from our sanitized path
        // so in a deep group hierarchy with long paths, room aliases are still distinct for rooms
        // derived from sibling groups.
        let mut path_start_index = 0;
        let maximum_allowed_path_length = 255 - 1 - 1 - self.homeserver_domain().len();
        if sanitized_path.len() > maximum_allowed_path_length {
            path_start_index = sanitized_path.len() - maximum_allowed_path_length
        }
        sanitized_path[path_start_index..].to_owned()
    }
}

#[async_trait::async_trait]
impl SynapseApi for SynapseClient {
    fn user_is_matrix_syncer(&self, matrix_user_id: &str) -> bool {
        matrix_user_id == self.config.matrix_syncer_user_id
    }

    fn homeserver_domain(&self) -> &str {
        let pos = self.config.matrix_syncer_user_id.find(":").unwrap() + 1;
        &self.config.matrix_syncer_user_id[pos..]
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3joined_rooms
    async fn get_joined_rooms_of_syncer(&mut self) -> Result<dto::JoinedRoomsResponse, KidsError> {
        self.client_api_get(ApiPath::from_segments(["joined_rooms"])).await
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#post_matrixclientv3roomsroomidleave
    async fn syncer_leave_room(&mut self, matrix_room_id: &str) -> Result<(), KidsError> {
        let _ = self
            .send_client_api_request::<(), dto::IgnoredResponse>(http::Method::POST, ApiPath::from_segments(["rooms", matrix_room_id, "leave"]), None)
            .await?;
        Ok(())
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#list-accounts-v3
    async fn get_users(&mut self) -> Result<dto::AllUsersResponse, KidsError> {
        let users: dto::AllUsersResponse = self
            .admin_api_get(
                "v3",
                ApiPath::from_segments_and_query(
                    ["users"],
                    [
                        ("limit", ALL_USERS.to_string().as_str()),
                        ("locked", "true"),
                        ("deactivated", "false"),
                    ],
                ),
            )
            .await?;
        Ok(users)
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#deactivate-account
    async fn deactivate_user(&mut self, matrix_user_id: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v1",
                http::Method::POST,
                ApiPath::from_segments(["deactivate", matrix_user_id]),
                Some(serde_json::json!({
                    "erase": true
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#query-user-account.
    async fn get_user_three_pids(&mut self, matrix_user_id: &str) -> Result<Vec<dto::ThreePID>, KidsError> {
        let response: dto::User = self.admin_api_get("v2", ApiPath::from_segments(["users", matrix_user_id])).await?;
        Ok(response.threepids.unwrap_or_default())
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#create-or-modify-account
    async fn set_user_three_pids(&mut self, matrix_user_id: &str, three_pids: &[dto::ThreePID]) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v2",
                http::Method::PUT,
                ApiPath::from_segments(["users", matrix_user_id]),
                Some(serde_json::json!({
                    "threepids": three_pids
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#create-or-modify-account
    async fn lock_user(&mut self, matrix_user_id: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v2",
                http::Method::PUT,
                ApiPath::from_segments(["users", matrix_user_id]),
                Some(serde_json::json!({
                    "locked": true
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#create-or-modify-account
    async fn unlock_user(&mut self, matrix_user_id: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v2",
                http::Method::PUT,
                ApiPath::from_segments(["users", matrix_user_id]),
                Some(serde_json::json!({
                    "locked": false
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3profileuseriddisplayname
    async fn set_user_display_name(&mut self, matrix_user_id: &str, display_name: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                ApiPath::from_segments(["profile", matrix_user_id, "displayname"]),
                Some(serde_json::json!({"displayname": display_name})),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3profileuseriddisplayname
    async fn get_user_display_name(&mut self, matrix_user_id: &str) -> Result<Option<String>, KidsError> {
        let response: dto::UserDisplayNameResponse = self.client_api_get(ApiPath::from_segments(["profile", matrix_user_id, "displayname"])).await?;
        Ok(response.display_name)
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#create-or-modify-account
    async fn create_user(&mut self, matrix_user_id: &str, source_user_id: &str) -> Result<dto::User, KidsError> {
        self.send_admin_api_request(
            "v2",
            http::Method::PUT,
            ApiPath::from_segments(["users", matrix_user_id]),
            Some(serde_json::json!({
                "external_ids": [
                    {
                        "auth_provider": self.config.matrix_source_oidc_provider_id,
                        "external_id": source_user_id
                    }
                ]
            })),
        )
        .await
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#post_matrixclientv3createroom
    async fn create_room(&mut self, name: &str, path: &str) -> Result<dto::RoomCreationResponse, KidsError> {
        self.send_client_api_request(
            http::Method::POST,
            ApiPath::from_segments(["createRoom"]),
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
    async fn delete_room(&mut self, matrix_room_id: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v1", // We are intentionally using the older, blocking version of the API here.
                http::Method::DELETE,
                ApiPath::from_segments(["rooms", matrix_room_id]),
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
        &mut self,
        matrix_room_id: &str,
        source_group_id: &kids_lib::types::SharedResourceIdentifier,
    ) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                ApiPath::from_segments(["rooms", matrix_room_id, "state", SYNCER_ROOM_METADATA_EVENT]),
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
    async fn get_room_associated_source_group_id(&mut self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError> {
        let event: dto::RoomGlobalIdEvent = self
            .client_api_get(ApiPath::from_segments(["rooms", matrix_room_id, "state", SYNCER_ROOM_METADATA_EVENT]))
            .await?;
        tracing::debug!(source_id = event.source_id, matrix_room_id, "Found mapping");
        Ok(event.source_id)
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3useruseridroomsroomidaccount_datatype
    /// Old version of storing syncer metadata for a room in the account data of the sync user
    /// instead of in the metadata of a room directly.
    async fn get_room_associated_source_group_id_v1(&mut self, matrix_room_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError> {
        let account_data_event: serde_json::Value = self
            .client_api_get(ApiPath::from_segments([
                "user",
                &self.config.matrix_syncer_user_id,
                "rooms",
                matrix_room_id,
                "account_data",
                &format!("{}.room_sync", self.config.matrix_namespace),
            ]))
            .await?;
        match account_data_event.get(format!("{}.room_sync.source_id", self.config.matrix_namespace)) {
            Some(val) => Ok(val.as_str().unwrap().to_string()),
            None => Err(KidsError::InternalError(
                "Old version of room sync event did not contain expected attribute".to_string(),
            )),
        }
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3roomsroomidstateeventtypestatekey
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomname
    async fn set_room_display_name(&mut self, matrix_room_id: &str, display_name: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                ApiPath::from_segments(["rooms", matrix_room_id, "state", "m.room.name"]),
                Some(&dto::RoomNameEvent {
                    name: display_name.to_string(),
                }),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3roomsroomideventeventid
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomname
    async fn get_room_display_name(&mut self, matrix_room_id: &str) -> Result<String, KidsError> {
        let room_name_event: dto::RoomNameEvent = self
            .client_api_get(ApiPath::from_segments(["rooms", matrix_room_id, "state", "m.room.name"]))
            .await?;
        Ok(room_name_event.name)
    }

    fn full_room_alias(&self, group_path: &str) -> String {
        "#".to_owned() + &self.room_alias_local_part(group_path) + ":" + self.homeserver_domain()
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3directoryroomroomalias.
    async fn create_room_alias(&mut self, matrix_room_id: &str, alias: &str) -> Result<(), KidsError> {
        let res: Result<dto::IgnoredResponse, KidsError> = self
            .send_client_api_request(
                http::Method::PUT,
                ApiPath::from_segments(["directory", "room", alias]),
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
    async fn delete_room_alias(&mut self, alias: &str) -> Result<(), KidsError> {
        let _ = self
            .send_client_api_request::<(), serde_json::Value>(http::Method::DELETE, ApiPath::from_segments(["directory", "room", alias]), None)
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#put_matrixclientv3roomsroomidstateeventtypestatekey
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomcanonical_alias
    async fn set_room_canonical_alias(&mut self, matrix_room_id: &str, canonical_alias: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                ApiPath::from_segments(["rooms", matrix_room_id, "state", "m.room.canonical_alias"]),
                Some(dto::RoomCanonicalAliasEvent {
                    alias: canonical_alias.to_owned(),
                    alt_aliases: None,
                }),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3roomsroomideventeventid
    /// Event type used is https://spec.matrix.org/v1.15/client-server-api/#mroomcanonical_alias
    async fn get_room_canonical_alias(&mut self, room_id: &str) -> Result<dto::RoomCanonicalAliasEvent, KidsError> {
        self.client_api_get(ApiPath::from_segments(["rooms", room_id, "state", "m.room.canonical_alias"]))
            .await
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#query-user-account.
    async fn get_source_user_id_for_matrix_user_id(&mut self, matrix_user_id: &str) -> Result<kids_lib::types::SharedResourceIdentifier, KidsError> {
        let response: dto::User = self.admin_api_get("v2", ApiPath::from_segments(["users", matrix_user_id])).await?;
        // This endpoint returns extended user information guaranteed to contain the external_ids field.
        for external_id in response.external_ids.unwrap() {
            if external_id.auth_provider == self.config.matrix_source_oidc_provider_id {
                return Ok(external_id.external_id);
            }
        }

        Err(KidsError::InternalError(format!(
            "Did not find external ID for source auth provider for matrix user: {matrix_user_id}"
        )))
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#list-joined-rooms-of-a-user
    async fn get_user_joined_rooms(&mut self, matrix_user_id: &str) -> Result<dto::UserJoinedRoomsResponse, KidsError> {
        self.admin_api_get("v1", ApiPath::from_segments(["users", matrix_user_id, "joined_rooms"]))
            .await
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#get_matrixclientv3roomsroomidjoined_members.
    async fn get_room_joined_users(&mut self, matrix_room_id: &str) -> Result<dto::RoomJoinedUsersResponse, KidsError> {
        self.client_api_get(ApiPath::from_segments(["rooms", matrix_room_id, "joined_members"])).await
    }

    /// See https://element-hq.github.io/synapse/latest/admin_api/room_membership.html.
    async fn join_user_to_room(&mut self, matrix_group_id: &str, matrix_user_id: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v1",
                http::Method::POST,
                ApiPath::from_segments(["join", matrix_group_id]),
                Some(&serde_json::json!({
                    "user_id": matrix_user_id
                })),
            )
            .await?;
        Ok(())
    }

    /// See https://spec.matrix.org/v1.15/client-server-api/#post_matrixclientv3roomsroomidkick.
    async fn kick_user_from_room(&mut self, matrix_group_id: &str, matrix_user_id: &str) -> Result<(), KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::POST,
                ApiPath::from_segments(["rooms", matrix_group_id, "kick"]),
                Some(serde_json::json!({
                    "user_id": matrix_user_id
                })),
            )
            .await?;
        Ok(())
    }
}
