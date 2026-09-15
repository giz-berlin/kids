/// Types used by the [MAS Admin API](https://element-hq.github.io/matrix-authentication-service/api/index.html).
pub mod mas {
    /// A response for a single element of type `TDataResponse`.
    ///
    /// `TDataResponse` should be one of the exposed type aliases in [`mas`](self).
    #[derive(Debug, serde::Deserialize)]
    pub struct SingleResponse<TDataResponse> {
        pub data: TDataResponse,
    }
    /// A response for a list of elements of type `TDataResponse`.
    ///
    /// `TDataResponse` should be one of the exposed type aliases in [`mas`](self).
    #[derive(Debug, serde::Deserialize)]
    pub struct ListResponse<TDataResponse> {
        pub data: Vec<TDataResponse>,
    }
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
        #[derive(Debug, serde::Deserialize)]
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
            };
        }
        pub mod upstream_oauth_link {
            make_id!();
            #[derive(Debug, serde::Deserialize)]
            pub struct Attributes {
                pub created_at: chrono::DateTime<chrono::Utc>,
                pub provider_id: super::upstream_oauth_provider::Id,
                pub subject: String,
                pub user_id: super::user::Id,
                pub human_account_name: String,
            }
        }
        pub mod upstream_oauth_provider {
            make_id!();
        }
        pub mod user {
            make_id!();
            #[derive(Debug, serde::Deserialize)]
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
            #[derive(Debug, serde::Deserialize)]
            pub struct Attributes {
                pub created_at: chrono::DateTime<chrono::Utc>,
                pub user_id: super::user::Id,
                pub email: String,
            }
        }
        pub mod type_enums {
            #[derive(Debug, serde::Deserialize)]
            #[serde(rename_all = "kebab-case")]
            pub enum UpstreamOauthLink {
                UpstreamOauthLink,
            }
            #[derive(Debug, serde::Deserialize)]
            #[serde(rename_all = "kebab-case")]
            pub enum User {
                User,
            }
            #[derive(Debug, serde::Deserialize)]
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

/// Types used by [Synapse's API](https://element-hq.github.io/synapse/latest/usage/administration/admin_api/index.html).
pub mod synapse {
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
    pub struct ExternalId {
        pub auth_provider: String,
        pub external_id: String,
    }

    /// A matrix user, as represented by Synapse.
    #[derive(serde::Deserialize, Debug, PartialEq, Clone)]
    pub struct User {
        /// Matrix user ID (named name here, what do I know why)
        pub name: String,
        pub locked: bool,
        pub external_ids: Option<Vec<ExternalId>>,
        pub threepids: Option<Vec<ThreePID>>,
    }
}

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
        pub joined: std::collections::HashMap<String, serde_json::Value>,
    }
}
