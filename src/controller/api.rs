use crate::{controller::state, source, target};

async fn serve_api(axum::Extension(api): axum::Extension<aide::openapi::OpenApi>) -> impl aide::axum::IntoApiResponse {
    axum::Json(api)
}

pub async fn run<S: source::interface::Source + Send + Sync + 'static, T: target::interface::Target + Send + Sync + 'static>(
    bind_addr: String,
    source: S,
    target: T,
) -> anyhow::Result<()> {
    // create metadata for API docs
    let mut api = aide::openapi::OpenApi {
        info: aide::openapi::Info {
            title: "Keycloak Identity Syncer".to_string(),
            description: Some(format!("Synchronizes users and groups. Source: {}, Target: {}", source.info(), target.info())),
            contact: Some(aide::openapi::Contact {
                name: Some("Leonard Marschke".to_string()),
                url: Some("https://rechenknecht.net/giz/keycloak/kids".to_string()),
                email: Some("leo@mixxplorer.de".to_string()),
                extensions: indexmap::IndexMap::new(),
            }),
            version: env!("CARGO_PKG_VERSION").to_string(),
            ..aide::openapi::Info::default()
        },
        ..aide::openapi::OpenApi::default()
    };

    let state = state::AppState {
        source: std::sync::Arc::new(source),
        target: std::sync::Arc::new(tokio::sync::RwLock::new(target)),
    };

    let app = aide::axum::ApiRouter::new()
        // Add routes of official API
        .api_route(
            "/v1/health",
            aide::axum::routing::get_with(crate::controller::handlers::health::health, crate::controller::handlers::health::health_desc),
        )
        .api_route(
            "/v1/users/{user_id}",
            aide::axum::routing::put_with(
                crate::controller::handlers::user::create_or_update_user,
                crate::controller::handlers::user::create_or_update_user_desc,
            ),
        )
        .api_route(
            "/v1/users/{user_id}",
            aide::axum::routing::delete_with(
                crate::controller::handlers::user::delete_user,
                crate::controller::handlers::user::delete_user_desc,
            ),
        )
        .api_route(
            "/v1/groups/{group_id}",
            aide::axum::routing::put_with(
                crate::controller::handlers::group::create_or_update_group,
                crate::controller::handlers::group::create_or_update_group_desc,
            ),
        )
        .api_route(
            "/v1/groups/{group_id}",
            aide::axum::routing::delete_with(
                crate::controller::handlers::group::delete_group,
                crate::controller::handlers::group::delete_group_desc,
            ),
        )
        .route("/docs/api.json", aide::axum::routing::get(serve_api))
        .route(
            "/docs",
            aide::redoc::Redoc::new("/docs/api.json").with_title("Keycloak Identity Syncer").axum_route(),
        )
        .route("/", aide::axum::routing::get(|| async { axum::response::Redirect::to("/docs") }))
        .with_state(state)
        .finish_api(&mut api)
        // Add aide (open API) extension layer
        .layer(axum::Extension(api))
        .layer(sentry::integrations::tower::SentryLayer::new_from_top())
        .layer(sentry::integrations::tower::SentryHttpLayer::new().enable_transaction());

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
