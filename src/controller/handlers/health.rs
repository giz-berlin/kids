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

pub async fn health<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
) -> Result<axum::Json<HealthResponse>, crate::controller::error::ControllerError>
where
    S: crate::source::interface::Source + Send,
    T: crate::target::interface::Target,
{
    Ok(HealthResponse {
        source: state.source.info(),
        target: state.target.read().await.info(),
    }
    .into())
}
