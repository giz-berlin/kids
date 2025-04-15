#[derive(thiserror::Error, Debug)]
pub enum KidsError {
    #[error("expected a value for attribute {0}")]
    MissingAttribute(String),
    #[error("Authentication failed")]
    AuthenticationFailure,
    #[error("Request failed with status code {0}")]
    HttpFailure(u16),
    #[error("Request failed (network)")]
    RequestFailure,
}
