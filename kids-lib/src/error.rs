pub const NO_CONTEXT: &str = "No additional context provided";

#[derive(thiserror::Error, Debug)]
pub enum KidsError {
    #[error("{0}: Authentication failed, status {1} for request {2}: {3}")]
    AuthenticationFailed(String, u16, String, #[source] anyhow::Error),
    #[error("{0}: Request failed ({1})")]
    RequestFailed(String, #[source] anyhow::Error),
    #[error("{0}: API Operation failed, status {1} for request {2}: {3}")]
    ApiOperationFailed(String, u16, String, #[source] anyhow::Error),
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl KidsError {
    pub fn with_context(self, context: &str) -> Self {
        let context = context.to_string();
        match self {
            KidsError::AuthenticationFailed(_, b, c, d) => KidsError::AuthenticationFailed(context, b, c, d),
            KidsError::ApiOperationFailed(_, b, c, d) => KidsError::ApiOperationFailed(context, b, c, d),
            KidsError::RequestFailed(_, b) => KidsError::RequestFailed(context, b),
            KidsError::InternalError(_) => KidsError::InternalError(context),
        }
    }
}
