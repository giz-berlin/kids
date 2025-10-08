use crate::{source, target};

pub fn delete_user_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Deletes the user in the target.").id("user/delete")
}

#[tracing::instrument(skip(state))]
pub async fn delete_user<S: source::interface::Source + Send, T: target::interface::Target>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S,T>>,
    axum::extract::Path((user_id,)): axum::extract::Path<(uuid::Uuid,)>,
) -> Result<axum::response::NoContent, crate::controller::error::WebAppError> {
    tracing::info!(user_id = tracing::field::display(user_id), source = state.source.info(), "Deleted user");

    Ok(axum::response::NoContent)
}

#[derive(serde::Serialize, schemars::JsonSchema, aide::OperationIo)]
#[serde(rename_all = "camelCase")]
#[aide(output)]
pub(crate) struct HealthResponse {
    source: String,
    target: String,
}

pub fn health_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Check API health.").id("health")
}

#[tracing::instrument(skip(state))]
pub async fn health<S: source::interface::Source + Send, T: target::interface::Target>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S,T>>,
) -> Result<axum::Json<HealthResponse>, crate::controller::error::WebAppError> {
    Ok(HealthResponse{
        source: state.source.info(),
        target: state.target.read().await.info(),
    }.into())
}
