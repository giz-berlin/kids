/// An aggregate representation of a Matrix user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The ID of the user used by Matrix, e.g. `@admin:giz.berlin`
    pub matrix_user_id: String,
    /// The ID of the user as used by [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub mas_user_id: crate::target::dto::mas::internal::user::Id,
    /// The ID of the user provided by the configured source, e.g. Keycloak.
    pub source_user_id: kids_lib::types::SharedResourceIdentifier,
    /// The display name of the user, if set.
    pub display_name: Option<String>,
    /// The email addresses associated with this account via [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub emails: Vec<String>,
    /// Whether the account is locked via [MAS](https://element-hq.github.io/matrix-authentication-service/index.html).
    pub locked: bool,
}
