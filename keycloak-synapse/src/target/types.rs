/// An aggregate representation of a Matrix user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The ID of the user used by Matrix, e.g. `@admin:giz.berlin`
    pub matrix_user_id: MatrixUserId,
    /// The ID of the user as used by [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub mas_user_id: crate::target::dto::mas::internal::user::Id,
    /// The ID of the user provided by the configured source, e.g. Keycloak, if available.
    pub source_user_id: Option<kids_lib::types::SharedResourceIdentifier>,
    /// The display name of the user, if set.
    pub display_name: Option<String>,
    /// The email addresses associated with this account via [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub emails: Vec<String>,
    /// Whether the account is locked via [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub locked: bool,
}

/// The server name of a Matrix homeserver, according to the [spec](https://spec.matrix.org/v1.19/appendices/#server-name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixServerName {
    hostname: url::Host,
    port: Option<u16>,
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
            let hostname = url::Host::parse(s).map_err(|err| <D::Error as serde::de::Error>::custom(format!("Error parsing hostname: {err}")))?;
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
        Ok(if let Some((host_part, port_part)) = host_with_port.split_once(':') {
            let hostname = url::Host::parse(host_part).map_err(|err| <D::Error as serde::de::Error>::custom(format!("Error parsing hostname: {err}")))?;
            let port = port_part
                .parse()
                .map_err(|err| <D::Error as serde::de::Error>::custom(format!("Error parsing port: {err}")))?;
            Self { hostname, port: Some(port) }
        } else {
            let hostname = url::Host::parse(&host_with_port).map_err(|err| <D::Error as serde::de::Error>::custom(format!("Error parsing hostname: {err}")))?;
            Self { hostname, port: None }
        })
    }
}

/// The ID of a user used by Matrix, representing e.g. `@admin:giz.berlin`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixUserId {
    pub username: String,
    pub homeserver: MatrixServerName,
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

impl<'de> serde::Deserialize<'de> for MatrixUserId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        todo!()
    }
}
