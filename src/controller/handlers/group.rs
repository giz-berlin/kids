pub fn create_or_update_group_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Creates or updates the group in the target.").id("group/create_or_update")
}

pub async fn create_or_update_group<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
    axum::extract::Json(payload): axum::extract::Json<S::GroupWebhookPayload>,
) -> Result<axum::response::NoContent, crate::controller::error::ControllerError>
where
    S: crate::source::interface::Source + Send,
    T: crate::target::interface::Target,
{
    let group = std::sync::Arc::new(state.source.group_from_webhook(payload));

    tracing::info!(group_id = tracing::field::display(group.id()), "Creating or updating group");
    tracing::debug!(group = tracing::field::debug(group.clone()));

    state.target.write().await.create_or_update_group(group).await?;

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
    S: crate::source::interface::Source + Send + Sync,
    T: crate::target::interface::Target + Send + Sync,
{
    tracing::info!(group_id = tracing::field::display(group_id.clone()), "Deleting group");

    let mut target = state.target.write().await;

    if !target.all_groups().await?.contains(&group_id) {
        return Err(crate::controller::error::ControllerError::new(
            None,
            Some("Group not found".to_owned()),
            Some(format!("Group with id {group_id} not found in target")),
            http::StatusCode::NOT_FOUND,
        ));
    }

    target.delete_group(group_id).await?;

    Ok(axum::response::NoContent)
}
