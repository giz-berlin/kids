pub struct ControllerError {
    id: String,
    error: Option<anyhow::Error>,
    public_message: Option<String>,
    private_message: Option<String>,
    status_code: axum::http::StatusCode,
}

impl ControllerError {
    pub fn new(
        error: Option<anyhow::Error>,
        public_message: Option<String>,
        private_message: Option<String>,
        status_code: axum::http::StatusCode,
    ) -> ControllerError {
        Self {
            id: crate::util::get_short_id(),
            error,
            public_message,
            private_message,
            status_code,
        }
    }
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct WebError {
    /// Error ID: Can be used by server admin to lookup exact error / backtrace
    id: String,

    /// Message shown to API user
    message: Option<String>,
}

// Tell axum how to convert `AppError` into a response.
impl axum::response::IntoResponse for ControllerError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!(
            error_id = self.id,
            code = self.status_code.to_string(),
            private_message = ?self.private_message,
            public_message = ?self.public_message,
            error = ?self.error,
            "Request failed (Status Code {})", self.status_code
        );
        (
            self.status_code,
            axum::Json(WebError {
                id: self.id,
                message: self.public_message,
            }),
        )
            .into_response()
    }
}

macro_rules! web_app_error_response {
    ($ctx:tt, $desc:tt, $message:tt) => {
        aide::openapi::Response {
            description: $desc.to_string(),
            content: indexmap::indexmap! {
                "application/json".to_owned() => aide::openapi::MediaType {
                    schema: Some(aide::openapi::SchemaObject {
                        json_schema: $ctx.schema.subschema_for::<WebError>(),
                        example: None,
                        external_docs: None,
                    }),
                    example: Some(serde_json::json!(WebError{ id: crate::util::get_short_id(), message: Some($message.to_string()) })),
                    ..Default::default()
                }
            },
            ..Default::default()
        }
    };
}

impl aide::OperationOutput for ControllerError {
    type Inner = String;

    fn inferred_responses(
        ctx: &mut aide::generate::GenContext,
        _operation: &mut aide::openapi::Operation,
    ) -> std::vec::Vec<(std::option::Option<u16>, aide::openapi::Response)> {
        vec![(
            Some(axum::http::StatusCode::INTERNAL_SERVER_ERROR.into()),
            web_app_error_response!(
                ctx,
                "Internal Server Error",
                "Example error message. Contact server admin to get more information."
            ),
        )]
    }
}

// Anyhow Error -> WebAppError, so we can just use anyhow for the most part
impl From<anyhow::Error> for ControllerError {
    fn from(err: anyhow::Error) -> Self {
        Self {
            id: crate::util::get_short_id(),
            private_message: Some(err.to_string()),
            error: Some(err),
            public_message: Some("Internal Server Error".into()),
            status_code: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<crate::error::KidsError> for ControllerError {
    fn from(err: crate::error::KidsError) -> Self {
        Self {
            id: crate::util::get_short_id(),
            private_message: Some(err.to_string()),
            error: Some(err.into()),
            public_message: Some("Internal Server Error".into()),
            status_code: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
