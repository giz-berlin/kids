use crate::target::synapse::dto;
use crate::{error, types};
use anyhow::anyhow;
use reqwest::RequestBuilder;

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

#[mockall::automock]
#[async_trait::async_trait(?Send)]
pub trait SynapseApi {
    fn user_is_matrix_syncer(&self, matrix_user_id: &str) -> bool;
    async fn get_joined_rooms_of_syncer(&mut self) -> Result<dto::JoinedRoomsResponse, error::KidsError>;
    async fn syncer_leave_room(&mut self, matrix_room_id: &str) -> Result<(), error::KidsError>;
    async fn get_users(&mut self) -> Result<dto::AllUsersResponse, error::KidsError>;
    async fn deactivate_user(&mut self, matrix_user_id: &str) -> Result<(), error::KidsError>;
    async fn lock_user(&mut self, matrix_user_id: &str) -> Result<(), error::KidsError>;
    async fn unlock_user(&mut self, matrix_user_id: &str) -> Result<(), error::KidsError>;

    async fn create_room(&mut self, name: &str, path: &str) -> Result<dto::RoomCreationResponse, error::KidsError>;
    async fn delete_room(&mut self, matrix_room_id: &str) -> Result<(), error::KidsError>;
    async fn associate_source_group_id_to_room(
        &mut self,
        matrix_room_id: &str,
        source_group_id: &types::SharedResourceIdentifier,
    ) -> Result<(), error::KidsError>;
    async fn get_room_associated_source_group_id(&mut self, matrix_room_id: &str) -> Result<types::SharedResourceIdentifier, error::KidsError>;
    async fn get_room_associated_source_group_id_v1(&mut self, matrix_room_id: &str) -> Result<types::SharedResourceIdentifier, error::KidsError>;
    async fn set_room_display_name(&mut self, matrix_room_id: &str, display_name: &str) -> Result<(), error::KidsError>;
    async fn get_room_display_name(&mut self, matrix_room_id: &str) -> Result<String, error::KidsError>;

    fn full_room_alias(&self, group_path: &str) -> String;
    async fn create_room_alias(&mut self, matrix_room_id: &str, alias: &str) -> Result<(), error::KidsError>;
    async fn delete_room_alias(&mut self, alias: &str) -> Result<(), error::KidsError>;
    async fn set_room_canonical_alias(&mut self, matrix_room_id: &str, canonical_alias: &str) -> Result<(), error::KidsError>;
    async fn get_room_canonical_alias(&mut self, matrix_room_id: &str) -> Result<dto::RoomCanonicalAliasEvent, error::KidsError>;

    async fn get_source_user_id_for_matrix_user_id(&mut self, matrix_user_id: &str) -> Result<types::SharedResourceIdentifier, error::KidsError>;
    async fn get_user_joined_rooms(&mut self, matrix_user_id: &str) -> Result<dto::UserJoinedRoomsResponse, error::KidsError>;
    async fn get_room_joined_users(&mut self, matrix_room_id: &str) -> Result<dto::RoomJoinedUsersResponse, error::KidsError>;
    async fn join_user_to_room(&mut self, matrix_room_id: &str, matrix_user_id: &str) -> Result<(), error::KidsError>;
    async fn kick_user_from_room(&mut self, matrix_room_id: &str, matrix_user_id: &str) -> Result<(), error::KidsError>;
}

pub struct SynapseClient {
    authentication: Authentication,
    config: SynapseApiConfig,
    http_client: reqwest::Client,
    parsed_homeserver_url: url::Url,
}

struct Authentication {
    access_token: String,
    refresh_token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// Indicates that we have not received an access token yet.
const NO_TOKEN: &str = "";
/// Page size requested when loading users.
/// Because we don't support pagination, this needs to be large enough to return all users
/// known to Synapse.
const ALL_USERS: u32 = u32::MAX;
/// The name of the room state event the syncer stores its metadata in
/// (such as the mapping of room to source group).
const SYNCER_ROOM_METADATA_EVENT: &str = "m.room.kids.room_sync";

impl SynapseClient {
    pub async fn new(config: SynapseApiConfig) -> Result<Self, error::KidsError> {
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
                access_token: NO_TOKEN.to_string(),
                refresh_token: NO_TOKEN.to_string(),
                expires_at: chrono::Utc::now(),
            },
            parsed_homeserver_url,
        };

        synapse_client.login().await?;

        Ok(synapse_client)
    }

    async fn login(&mut self) -> Result<(), error::KidsError> {
        let token_response: dto::MatrixAuthentication = self
            .send_client_api_request_unauthenticated(
                http::Method::POST,
                "login".to_string(),
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
        self.authentication.access_token = token_response.access_token;
        self.authentication.refresh_token = token_response.refresh_token;
        self.authentication.expires_at = chrono::Utc::now() + chrono::Duration::milliseconds(token_response.expires_in_ms);

        tracing::info!(homeserver_url=%self.parsed_homeserver_url, "Logged in to homeserver");
        Ok(())
    }

    async fn refresh_access_token_if_necessary(&mut self) -> Result<(), error::KidsError> {
        // In order to avoid the access token expiring between this check and the actual request,
        // we also refresh tokens that are not yet expired but will be soon.
        if self.authentication.expires_at - chrono::Duration::seconds(5) < chrono::Utc::now() {
            tracing::debug!("Refreshing access token");
            let token_response: dto::MatrixAuthentication = self
                .send_client_api_request_unauthenticated(
                    http::Method::POST,
                    "refresh".to_string(),
                    Some(serde_json::json!({
                        "refresh_token": self.authentication.refresh_token,
                    })),
                )
                .await?;
            self.authentication.access_token = token_response.access_token;
            self.authentication.refresh_token = token_response.refresh_token;
            self.authentication.expires_at = chrono::Utc::now() + chrono::Duration::milliseconds(token_response.expires_in_ms);
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
    ) -> Result<RequestBuilder, error::KidsError> {
        let mut builder = self.construct_unauthenticated_request(method, url, body);
        self.refresh_access_token_if_necessary().await?;
        builder = builder.bearer_auth(self.authentication.access_token.clone());
        Ok(builder)
    }

    async fn send_request<T: serde::de::DeserializeOwned>(&mut self, request: RequestBuilder) -> Result<T, error::KidsError> {
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let url = response.url().to_string();
                if status.is_success() {
                    return match response.json().await {
                        Ok(json) => Ok(json),
                        Err(error) => Err(error::KidsError::ApiOperationFailed(
                            error::NO_CONTEXT.to_string(),
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
                    return Err(error::KidsError::AuthenticationFailed(
                        error::NO_CONTEXT.to_string(),
                        status.as_u16(),
                        url,
                        anyhow!(error_information),
                    ));
                }

                Err(error::KidsError::ApiOperationFailed(
                    error::NO_CONTEXT.to_string(),
                    status.as_u16(),
                    url,
                    anyhow!(error_information),
                ))
            }
            Err(e) => Err(error::KidsError::RequestFailed(error::NO_CONTEXT.to_string(), anyhow!(e))),
        }
    }

    async fn send_client_api_request_unauthenticated<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        method: http::Method,
        path: String,
        body: Option<B>,
    ) -> Result<T, error::KidsError> {
        let request = self.construct_unauthenticated_request(method, format!("{}_matrix/client/v3/{}", self.parsed_homeserver_url, path), body);
        self.send_request(request).await
    }

    async fn send_client_api_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        method: http::Method,
        path: String,
        body: Option<B>,
    ) -> Result<T, error::KidsError> {
        let request = self
            .construct_authenticated_request(method, format!("{}_matrix/client/v3/{}", self.parsed_homeserver_url, path), body)
            .await?;
        self.send_request(request).await
    }

    async fn client_api_get<T: serde::de::DeserializeOwned>(&mut self, path: String) -> Result<T, error::KidsError> {
        self.send_client_api_request::<(), T>(http::Method::GET, path, None).await
    }

    async fn send_admin_api_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        api_version: &str,
        method: http::Method,
        path: String,
        body: Option<B>,
    ) -> Result<T, error::KidsError> {
        let request = self
            .construct_authenticated_request(method, format!("{}_synapse/admin/{}/{}", self.parsed_homeserver_url, api_version, path), body)
            .await?;
        self.send_request(request).await
    }

    async fn admin_api_get<T: serde::de::DeserializeOwned>(&mut self, api_version: &str, path: String) -> Result<T, error::KidsError> {
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

    fn homeserver_domain(&self) -> &str {
        let pos = self.config.matrix_syncer_user_id.find(":").unwrap() + 1;
        &self.config.matrix_syncer_user_id[pos..]
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

#[async_trait::async_trait(?Send)]
impl SynapseApi for SynapseClient {
    fn user_is_matrix_syncer(&self, matrix_user_id: &str) -> bool {
        matrix_user_id == self.config.matrix_syncer_user_id
    }

    async fn get_joined_rooms_of_syncer(&mut self) -> Result<dto::JoinedRoomsResponse, error::KidsError> {
        self.client_api_get("joined_rooms".to_string()).await
    }

    async fn syncer_leave_room(&mut self, matrix_room_id: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_client_api_request::<(), dto::IgnoredResponse>(http::Method::POST, format!("rooms/{matrix_room_id}/leave"), None)
            .await?;
        Ok(())
    }

    async fn get_users(&mut self) -> Result<dto::AllUsersResponse, error::KidsError> {
        let users: dto::AllUsersResponse = self
            .admin_api_get("v2", format!("users?limit={ALL_USERS}&locked=true&deactivated=false"))
            .await?;
        Ok(users)
    }

    async fn deactivate_user(&mut self, matrix_user_id: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v1",
                http::Method::POST,
                format!("deactivate/{matrix_user_id}"),
                Some(serde_json::json!({
                    "erase": true
                })),
            )
            .await?;
        Ok(())
    }

    async fn lock_user(&mut self, matrix_user_id: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v2",
                http::Method::PUT,
                format!("users/{matrix_user_id}"),
                Some(serde_json::json!({
                    "locked": true
                })),
            )
            .await?;
        Ok(())
    }

    async fn unlock_user(&mut self, matrix_user_id: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v2",
                http::Method::PUT,
                format!("users/{matrix_user_id}"),
                Some(serde_json::json!({
                    "locked": false
                })),
            )
            .await?;
        Ok(())
    }

    async fn create_room(&mut self, name: &str, path: &str) -> Result<dto::RoomCreationResponse, error::KidsError> {
        self.send_client_api_request(
            http::Method::POST,
            "createRoom".to_string(),
            Some(serde_json::json!({
                "name": name,
                "visibility": "private",
                "preset": "private_chat",
                "initial_state": self.static_room_initial_state(),
                "power_level_content_override": self.static_room_power_level_content_override(),
                "room_alias_name": self.room_alias_local_part(path)
            })),
        )
        .await
    }

    async fn delete_room(&mut self, matrix_room_id: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v2",
                http::Method::DELETE,
                format!("rooms/{matrix_room_id}"),
                Some(&serde_json::json!({
                    "purge": true // Deletes all traces of the room from the database.
                })),
            )
            .await?;
        Ok(())
    }

    async fn associate_source_group_id_to_room(
        &mut self,
        matrix_room_id: &str,
        source_group_id: &types::SharedResourceIdentifier,
    ) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                format!("rooms/{matrix_room_id}/state/{SYNCER_ROOM_METADATA_EVENT}/"),
                Some(serde_json::json!({
                    "source_id": source_group_id
                })),
            )
            .await?;
        Ok(())
    }

    async fn get_room_associated_source_group_id(&mut self, matrix_room_id: &str) -> Result<types::SharedResourceIdentifier, error::KidsError> {
        let event: dto::RoomGlobalIdEvent = self.client_api_get(format!("rooms/{matrix_room_id}/state/{SYNCER_ROOM_METADATA_EVENT}/") ).await?;
        tracing::debug!(source_id = event.source_id, matrix_room_id, "Found mapping");
        Ok(event.source_id)
    }

    async fn get_room_associated_source_group_id_v1(&mut self, matrix_room_id: &str) -> Result<types::SharedResourceIdentifier, error::KidsError> {
        let account_data_event: serde_json::Value = self
            .client_api_get(format!(
                "user/{}/rooms/{}/account_data/{}.room_sync",
                self.config.matrix_syncer_user_id, matrix_room_id, self.config.matrix_namespace
            ))
            .await?;
        match account_data_event.get(format!("{}.room_sync.source_id", self.config.matrix_namespace)) {
            Some(val) => Ok(val.as_str().unwrap().to_string()),
            None => Err(error::KidsError::InternalError(
                "Old version of room sync event did not contain expected attribute".to_string(),
            )),
        }
    }

    async fn set_room_display_name(&mut self, matrix_room_id: &str, display_name: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                format!("rooms/{matrix_room_id}/state/m.room.name/"),
                Some(&dto::RoomNameEvent {
                    name: display_name.to_string(),
                }),
            )
            .await?;
        Ok(())
    }

    async fn get_room_display_name(&mut self, matrix_room_id: &str) -> Result<String, error::KidsError> {
        let room_name_event: dto::RoomNameEvent = self.client_api_get(format!("rooms/{matrix_room_id}/state/m.room.name/")).await?;
        Ok(room_name_event.name)
    }

    fn full_room_alias(&self, group_path: &str) -> String {
        "#".to_owned() + &self.room_alias_local_part(group_path) + ":" + self.homeserver_domain()
    }

    async fn create_room_alias(&mut self, matrix_room_id: &str, alias: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                format!("directory/room/{}", url::form_urlencoded::byte_serialize(alias.as_bytes()).collect::<String>()),
                Some(&serde_json::json!({
                    "room_id": matrix_room_id
                })),
            )
            .await?;
        Ok(())
    }

    async fn delete_room_alias(&mut self, alias: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_client_api_request::<(), serde_json::Value>(
                http::Method::DELETE,
                format!("directory/room/{}", url::form_urlencoded::byte_serialize(alias.as_bytes()).collect::<String>()),
                None,
            )
            .await?;
        Ok(())
    }

    async fn set_room_canonical_alias(&mut self, matrix_room_id: &str, canonical_alias: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::PUT,
                format!("rooms/{matrix_room_id}/state/m.room.canonical_alias/"),
                Some(dto::RoomCanonicalAliasEvent {
                    alias: canonical_alias.to_owned(),
                    alt_aliases: None,
                }),
            )
            .await?;
        Ok(())
    }

    async fn get_room_canonical_alias(&mut self, room_id: &str) -> Result<dto::RoomCanonicalAliasEvent, error::KidsError> {
        self.client_api_get(format!("rooms/{room_id}/state/m.room.canonical_alias/")).await
    }

    async fn get_source_user_id_for_matrix_user_id(&mut self, matrix_user_id: &str) -> Result<types::SharedResourceIdentifier, error::KidsError> {
        let response: dto::User = self.admin_api_get("v2", format!("users/{matrix_user_id}")).await?;
        // This endpoint returns extended user information guaranteed to contain the external_ids field.
        for external_id in response.external_ids.unwrap() {
            if external_id.auth_provider == self.config.matrix_source_oidc_provider_id {
                return Ok(external_id.external_id);
            }
        }

        Err(error::KidsError::InternalError(format!(
            "Did not find external ID for source auth provider for matrix user: {matrix_user_id}"
        )))
    }

    async fn get_user_joined_rooms(&mut self, matrix_user_id: &str) -> Result<dto::UserJoinedRoomsResponse, error::KidsError> {
        self.admin_api_get("v1", format!("users/{matrix_user_id}/joined_rooms")).await
    }

    async fn get_room_joined_users(&mut self, matrix_room_id: &str) -> Result<dto::RoomJoinedUsersResponse, error::KidsError> {
        self.client_api_get(format!("rooms/{matrix_room_id}/joined_members")).await
    }

    async fn join_user_to_room(&mut self, matrix_group_id: &str, matrix_user_id: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_admin_api_request(
                "v1",
                http::Method::POST,
                format!("join/{matrix_group_id}"),
                Some(&serde_json::json!({
                    "user_id": matrix_user_id
                })),
            )
            .await?;
        Ok(())
    }

    async fn kick_user_from_room(&mut self, matrix_group_id: &str, matrix_user_id: &str) -> Result<(), error::KidsError> {
        let _: dto::IgnoredResponse = self
            .send_client_api_request(
                http::Method::POST,
                format!("rooms/{matrix_group_id}/kick"),
                Some(serde_json::json!({
                    "user_id": matrix_user_id
                })),
            )
            .await?;
        Ok(())
    }
}
