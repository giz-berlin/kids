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

/// The ID of a user used by Matrix, representing e.g. `@admin:giz.berlin`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixUserId {
    pub username: String,
    pub homeserver: String,
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
