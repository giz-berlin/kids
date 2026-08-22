use std::collections;

/// This struct contains no fields. When deserializing to this type, we intentionally throw
/// away all information contained in the original string.
#[derive(serde::Deserialize)]
pub struct IgnoredResponse {}

#[derive(serde::Deserialize)]
pub struct MatrixAuthentication {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in_ms: Option<i64>,
}

#[derive(serde::Deserialize, Debug)]
pub struct AllUsersResponse {
    pub users: Vec<User>,
}

/// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#create-or-modify-account
/// for details of this enum.
#[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq, Eq, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ThreePIDMedium {
    Email,
    Msisdn,
}

/// See https://element-hq.github.io/synapse/latest/admin_api/user_admin_api.html#create-or-modify-account
/// for details of this struct.
#[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq, Eq, Clone)]
pub struct ThreePID {
    pub medium: ThreePIDMedium,
    pub address: String,
}

#[derive(serde::Deserialize, Debug, PartialEq, Clone)]
pub struct User {
    pub name: String,
    pub locked: bool,
    pub external_ids: Option<Vec<ExternalId>>,
    pub threepids: Option<Vec<ThreePID>>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct UserDisplayNameResponse {
    #[serde(rename = "displayname")]
    pub display_name: Option<String>,
}

#[derive(serde::Deserialize, Debug, PartialEq, Clone)]
pub struct ExternalId {
    pub auth_provider: String,
    pub external_id: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct JoinedRoomsResponse {
    pub joined_rooms: Vec<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct RoomCreationResponse {
    pub room_id: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct RoomGlobalIdEvent {
    pub source_id: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct RoomNameEvent {
    pub name: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct RoomCanonicalAliasEvent {
    pub alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_aliases: Option<Vec<String>>,
}

#[derive(serde::Deserialize, Debug)]
pub struct UserJoinedRoomsResponse {
    pub joined_rooms: Vec<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct RoomJoinedUsersResponse {
    pub joined: collections::HashMap<String, serde_json::Value>,
}
