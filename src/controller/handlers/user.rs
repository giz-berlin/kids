#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ListUsersDTO {
    users: std::collections::HashSet<crate::types::SharedResourceIdentifier>,
}

pub fn list_users_desc(op: aide::transform::TransformOperation) -> aide::transform::TransformOperation {
    op.description("Lists all users currently known to the target.").id("user/list")
}

pub async fn list_users<S, T>(
    axum::extract::State(state): axum::extract::State<crate::controller::state::AppState<S, T>>,
) -> Result<axum::response::Json<ListUsersDTO>, crate::controller::error::ControllerError>
where
    S: crate::source::interface::Source + Send,
    T: crate::target::interface::Target,
{
    let users = state.target.write().await.all_users().await?;

    Ok(axum::response::Json(ListUsersDTO { users }))
}

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
    let mut target = state.target.write().await;

    tracing::info!(user_id = tracing::field::display(user_id.clone()), "Deleting user");

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
