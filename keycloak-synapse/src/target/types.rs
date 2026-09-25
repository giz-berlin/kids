#[derive(Debug, serde::Serialize, Hash, Clone, PartialEq, Eq)]
pub struct AccessTokenScope(pub String);
impl std::fmt::Display for AccessTokenScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
/// The scope used to access Matrix routes (`_matrix/client/v3`).
///
/// Is `urn:matrix:client:api:*`.
pub fn matrix_api_scope() -> AccessTokenScope {
    AccessTokenScope("urn:synapse:admin:* urn:matrix:client:api:*".to_owned())
}
/// The scope used to access Synapse routes (`_synapse/admin`).
///
/// Is `urn:synapse:admin:*`.
pub fn synapse_api_scope() -> AccessTokenScope {
    AccessTokenScope("urn:synapse:admin:* urn:matrix:client:api:*".to_owned())
}

#[derive(Hash, Clone, PartialEq, Eq)]
pub struct AccessTokenToken(pub String);

/// An [access token](AccessTokenToken) with attached metadata ([scope](AccessTokenScope) and [validity](AccessToken::expires_at)).
#[derive(Clone)]
pub struct AccessToken {
    pub access_token: AccessTokenToken,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    #[allow(unused)]
    pub scope: AccessTokenScope,
    pub id: crate::target::dto::mas::internal::personal_session::Id,
}

/// An aggregate representation of a Matrix user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The ID of the user used by Matrix, e.g. `@admin:giz.berlin`
    pub matrix_user_id: MatrixUserId,
    /// The ID of the user as used by [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub mas_user_id: crate::target::dto::mas::internal::user::Id,
    /// The ID of the user provided by the configured source, e.g. Keycloak, if available.
    pub source_user_id: Option<kids_lib::types::SharedResourceIdentifier>,
    pub state: UserState,
}

/// This encapsulates all data of a [Matrix User](User) that we support changing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserState {
    /// The display name of the user, if set.
    pub display_name: Option<String>,
    /// The email addresses associated with this account via [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub emails: Vec<String>,
    /// Whether the account is locked via [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub locked: bool,
    /// Whether the account can request admin, i.e., can start sessions with admin privileges.
    pub is_admin: bool,
    /// The rooms this user is a member of.
    pub rooms: Vec<String>,
}

/// The server name of a Matrix homeserver, according to the [spec](https://spec.matrix.org/v1.19/appendices/#server-name).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatrixServerName {
    hostname: url::Host,
    port: Option<u16>,
}

impl MatrixServerName {
    pub fn display(&self) -> String {
        format!("{self}")
    }
}

impl std::fmt::Display for MatrixServerName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.port {
            Some(port) => write!(f, "{}:{port}", self.hostname),
            None => write!(f, "{}", self.hostname),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MatrixServerNameParseError {
    #[error("Error parsing hostname: {0}")]
    Host(#[from] url::ParseError),
    #[error("Error parsing port: {0}")]
    Port(#[from] core::num::ParseIntError),
}

impl std::str::FromStr for MatrixServerName {
    type Err = MatrixServerNameParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if let Some((host_part, port_part)) = s.split_once(':') {
            let hostname = url::Host::parse(host_part)?;
            let port = port_part.parse()?;
            Self { hostname, port: Some(port) }
        } else {
            let hostname = url::Host::parse(s)?;
            Self { hostname, port: None }
        })
    }
}

impl<'de> serde::Deserialize<'de> for MatrixServerName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let host_with_port = String::deserialize(deserializer)?;
        std::str::FromStr::from_str(&host_with_port).map_err(|err| <D::Error as serde::de::Error>::custom(format!("{err}")))
    }
}

/// The ID of a user used by Matrix, representing e.g. `@admin:giz.berlin`
///
/// This uses [4.3.1.1](https://spec.matrix.org/v1.19/appendices/#historical-user-ids) as reference for parsing rules:
/// It allows all non-surrogate Unicode codepoints except `\0` and `:` as part of the username.
/// See Matrix spec Appendix [4.3.1](https://spec.matrix.org/v1.19/appendices/#user-identifiers) and [4.3.1.1](https://spec.matrix.org/v1.19/appendices/#historical-user-ids) for context.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatrixUserId {
    pub username: String,
    pub homeserver: MatrixServerName,
}

impl MatrixUserId {
    pub fn display(&self) -> String {
        format!("{self}")
    }
}

impl std::fmt::Display for MatrixUserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{}:{}", self.username, self.homeserver)
    }
}

impl serde::Serialize for MatrixUserId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(&format!("{self}"), serializer)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MatrixUsernameParseError {
    #[error("The username does not start with a '@'.")]
    DoesNotStartWithAt(),
    #[error("Found an illegal symbol in the username: '{0}'")]
    HasIllegalSymbol(char),
}

#[derive(Debug, thiserror::Error)]
pub enum MatrixUserIdParseError {
    #[error("Error parsing username: {0}")]
    Username(#[from] MatrixUsernameParseError),
    #[error("Error parsing homeserver: {0}")]
    Homeserver(#[from] MatrixServerNameParseError),
    #[error("No homeserver part detected.")]
    NoHomeserver(),
}

impl std::str::FromStr for MatrixUserId {
    type Err = MatrixUserIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if let Some((username_part, homeserver_part)) = s.split_once(':') {
            let username = if let Some(stripped) = username_part.strip_prefix('@') {
                stripped
            } else {
                return Err(MatrixUsernameParseError::DoesNotStartWithAt().into());
            };
            if username.contains(':') {
                return Err(MatrixUsernameParseError::HasIllegalSymbol(':').into());
            }
            if username.contains('\0') {
                return Err(MatrixUsernameParseError::HasIllegalSymbol('\0').into());
            }
            let homeserver = std::str::FromStr::from_str(homeserver_part)?;
            Self {
                username: username.into(),
                homeserver,
            }
        } else {
            return Err(MatrixUserIdParseError::NoHomeserver());
        })
    }
}

impl<'de> serde::Deserialize<'de> for MatrixUserId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let username_with_homeserver = String::deserialize(deserializer)?;
        std::str::FromStr::from_str(&username_with_homeserver).map_err(|err| <D::Error as serde::de::Error>::custom(format!("{err}")))
    }
}

#[cfg(test)]
impl<S: Into<String>> From<S> for MatrixServerName {
    fn from(value: S) -> Self {
        let value = value.into();
        std::str::FromStr::from_str(&value).unwrap()
    }
}

#[cfg(test)]
impl<S: Into<String>> From<S> for MatrixUserId {
    fn from(value: S) -> Self {
        let value = value.into();
        std::str::FromStr::from_str(&value).unwrap()
    }
}

#[cfg(test)]
impl Default for MatrixUserId {
    fn default() -> Self {
        Self {
            username: "".to_owned(),
            homeserver: MatrixServerName {
                hostname: url::Host::Ipv4(core::net::Ipv4Addr::from_bits(0)),
                port: None,
            },
        }
    }
}
