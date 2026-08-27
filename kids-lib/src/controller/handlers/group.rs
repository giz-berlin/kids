#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ListGroupsDTO {
    groups: std::collections::HashSet<crate::types::SharedResourceIdentifier>,
}

pub fn list_groups_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Lists all groups currently known to the target.").id("group/list")
}

pub async fn list_groups<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
) -> Result<axum::response::Json<ListGroupsDTO>, crate::controller::error::ControllerError>
where
    S: crate::interface::source::Source + Send,
    T: crate::interface::target::Target,
{
    let groups = state.target.write().await.all_groups().await?;

    Ok(axum::response::Json(ListGroupsDTO { groups }))
}

pub fn create_or_update_group_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Creates or updates the group in the target.").id("group/create_or_update")
}

pub async fn create_or_update_group<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
    axum::extract::Json(payload): axum::extract::Json<S::GroupWebhookPayload>,
) -> Result<axum::response::NoContent, crate::controller::error::ControllerError>
where
    S: crate::interface::source::Source + Send,
    T: crate::interface::target::Target,
{
    let group: std::sync::Arc<dyn crate::interface::source::Group + Send + Sync> = std::sync::Arc::from(state.source.group_from_webhook(payload));

    let mut target = state.target.write().await;

    tracing::info!(group_id = tracing::field::display(group.id()), "Creating or updating group");
    tracing::debug!(group = tracing::field::debug(group.clone()));

    target.create_or_update_group(group).await?;

    Ok(axum::response::NoContent)
}

pub fn delete_group_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Deletes the group in the target.").id("group/delete")
}

pub async fn delete_group<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
    axum::extract::Path((group_id,)): axum::extract::Path<(crate::types::SharedResourceIdentifier,)>,
) -> Result<axum::response::NoContent, crate::controller::error::ControllerError>
where
    S: crate::interface::source::Source + Send + Sync,
    T: crate::interface::target::Target + Send + Sync,
{
    let mut target = state.target.write().await;

    tracing::info!(group_id = tracing::field::display(&group_id), "Deleting group");

    if !target.all_groups().await?.contains(&group_id) {
        return Err(crate::controller::error::ControllerError::new(
            None,
            Some("Group not found".to_owned()),
            Some(format!("Group with id {group_id} not found in target")),
            http::StatusCode::NOT_FOUND,
        ));
    }

    target.delete_group(&group_id).await?;

    Ok(axum::response::NoContent)
}
