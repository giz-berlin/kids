pub fn create_or_update_user_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Creates or updates the user in the target.").id("user/create_or_update")
}

pub async fn create_or_update_user<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
    axum::extract::Json(payload): axum::extract::Json<S::UserWebhookPayload>,
) -> Result<axum::response::NoContent, crate::controller::error::ControllerError>
where
    S: crate::source::interface::Source + Send,
    T: crate::target::interface::Target,
{
    let user = std::sync::Arc::new(state.source.user_from_webhook(payload));

    let mut target = state.target.write().await;

    tracing::info!(user_id = tracing::field::display(user.id()), "Creating or updating user");
    tracing::debug!(user = tracing::field::debug(user.clone()));

    target.create_or_update_user(user).await?;

    Ok(axum::response::NoContent)
}

pub fn delete_user_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Deletes the user in the target.").id("user/delete")
}

pub async fn delete_user<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
    axum::extract::Path((user_id,)): axum::extract::Path<(crate::types::SharedResourceIdentifier,)>,
) -> Result<axum::response::NoContent, crate::controller::error::ControllerError>
where
    S: crate::source::interface::Source + Send,
    T: crate::target::interface::Target,
{
    tracing::info!(user_id = tracing::field::display(user_id.clone()), "Deleting user");

    let mut target = state.target.write().await;

    if !target.all_users().await?.contains(&user_id) {
        return Err(crate::controller::error::ControllerError::new(
            None,
            Some("User not found".to_owned()),
            Some(format!("User with id {user_id} not found in target")),
            http::StatusCode::NOT_FOUND,
        ));
    }

    target.delete_user(user_id.clone()).await?;

    Ok(axum::response::NoContent)
}
