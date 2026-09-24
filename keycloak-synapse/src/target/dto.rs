/// Types used by the [MAS Admin API](https://element-hq.github.io/matrix-authentication-service/api/index.html).
pub mod mas {
    /// A response for a single element of type `TDataResponse`.
    ///
    /// `TDataResponse` should be one of the exposed type aliases in [`mas`](self).
    #[derive(Debug, serde::Deserialize)]
    pub struct SingleResponse<TDataResponse> {
        pub data: TDataResponse,
    }
    #[derive(Debug, serde::Deserialize)]
    pub struct Meta {
        pub count: u64,
    }
    /// A response for a list of elements of type `TDataResponse`.
    ///
    /// `TDataResponse` should be one of the exposed type aliases in [`mas`](self).
    #[derive(Debug, serde::Deserialize)]
    pub struct ListResponse<TDataResponse> {
        pub meta: Meta,
        pub data: Vec<TDataResponse>,
    }
    /// A response of type `personal-session`.
    pub type PersonalSessionResponse =
        internal::DataResponse<internal::type_enums::PersonalSession, internal::personal_session::Id, internal::personal_session::Attributes>;
    /// A response of type `upstream-oauth-link`.
    pub type UpstreamOauthLinkResponse =
        internal::DataResponse<internal::type_enums::UpstreamOauthLink, internal::upstream_oauth_link::Id, internal::upstream_oauth_link::Attributes>;
    /// A response of type `user`.
    pub type UserResponse = internal::DataResponse<internal::type_enums::User, internal::user::Id, internal::user::Attributes>;
    /// A response of type `user-email`.
    pub type UserEmailResponse = internal::DataResponse<internal::type_enums::UserEmail, internal::user_email::Id, internal::user_email::Attributes>;
    /// Module containing types exposed by type aliases in the [super module `mas`](self).
    // We allow unused here, as we represent the types returned by the API, regardless of whether they are used in KIDS.
    #[allow(unused)]
    pub mod internal {
        #[derive(Debug, serde::Deserialize, Clone)]
        pub struct DataResponse<Type, Id, Attributes> {
            pub r#type: Type,
            pub id: Id,
            pub attributes: Attributes,
            // `links`` and `meta` not included.
        }
        macro_rules! make_id {
            () => {
                #[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
                pub struct Id(String);
                impl std::fmt::Display for Id {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        std::fmt::Display::fmt(&self.0, f)
                    }
                }
                impl Id {
                    pub const fn as_str(&self) -> &str {
                        self.0.as_str()
                    }
                }
                #[cfg(test)]
                impl<T: Into<String>> From<T> for Id {
                    fn from(value: T) -> Self {
                        Self(value.into())
                    }
                }
                #[cfg(test)]
                impl Default for Id {
                    fn default() -> Self {
                        Self(kids_lib::util::get_short_id())
                    }
                }
            };
        }
        pub mod personal_session {
            make_id!();
            #[derive(Debug, serde::Deserialize, Clone)]
            pub struct Attributes {
                pub created_at: chrono::DateTime<chrono::Utc>,
                pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
                pub owner_user_id: Option<super::user::Id>,
                pub owner_client_id: Option<String>,
                pub actor_user_id: super::user::Id,
                pub human_name: String,
                pub scope: String,
                pub last_active_at: Option<chrono::DateTime<chrono::Utc>>,
                pub last_active_ip: Option<String>,
                pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
                pub access_token: String,
            }
        }
        pub mod upstream_oauth_link {
            make_id!();
            #[derive(Debug, serde::Deserialize, Clone)]
            pub struct Attributes {
                pub created_at: chrono::DateTime<chrono::Utc>,
                pub provider_id: super::upstream_oauth_provider::Id,
                pub subject: String,
                pub user_id: super::user::Id,
                pub human_account_name: Option<String>,
            }
        }
        pub mod upstream_oauth_provider {
            make_id!();
        }
        pub mod user {
            make_id!();
            #[derive(Debug, serde::Deserialize, Clone)]
            pub struct Attributes {
                pub username: String,
                pub created_at: chrono::DateTime<chrono::Utc>,
                pub locked_at: Option<chrono::DateTime<chrono::Utc>>,
                pub deactivated_at: Option<chrono::DateTime<chrono::Utc>>,
                pub admin: bool,
                pub legacy_guest: bool,
            }
        }
        pub mod user_email {
            make_id!();
            #[derive(Debug, serde::Deserialize, Clone)]
            pub struct Attributes {
                pub created_at: chrono::DateTime<chrono::Utc>,
                pub user_id: super::user::Id,
                pub email: String,
            }
        }
        pub mod type_enums {
            #[derive(Debug, serde::Deserialize, Clone, Copy)]
            #[serde(rename_all = "kebab-case")]
            pub enum PersonalSession {
                PersonalSession,
            }
            #[derive(Debug, serde::Deserialize, Clone, Copy)]
            #[serde(rename_all = "kebab-case")]
            pub enum UpstreamOauthLink {
                UpstreamOauthLink,
            }
            #[derive(Debug, serde::Deserialize, Clone, Copy)]
            #[serde(rename_all = "kebab-case")]
            pub enum User {
                User,
            }
            #[derive(Debug, serde::Deserialize, Clone, Copy)]
            #[serde(rename_all = "kebab-case")]
            pub enum UserEmail {
                UserEmail,
            }
        }
    }
}

/// This struct contains no fields. When deserializing to this type, we intentionally throw
/// away all information contained in the original string.
#[derive(serde::Deserialize)]
pub struct IgnoredResponse {}

/// Types used by [Matrix's Client-Server (C-S) API](https://spec.matrix.org/latest/client-server-api/).
pub mod matrix {
    #[derive(serde::Deserialize, serde::Serialize, Debug)]
    pub struct UserDisplayNameResponse {
        #[serde(rename = "displayname")]
        pub display_name: Option<String>,
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
        pub joined: std::collections::HashMap<crate::target::types::MatrixUserId, serde_json::Value>,
    }
}
