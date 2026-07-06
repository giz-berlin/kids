use crate::{
    config,
    controller::{state, tls},
    source, target,
};

async fn serve_api(axum::Extension(api): axum::Extension<aide::openapi::OpenApi>) -> impl aide::axum::IntoApiResponse {
    axum::Json(api)
}

pub async fn run<S: source::interface::Source + Send + Sync + 'static, T: target::interface::Target + Send + Sync + 'static>(
    bind_addr: String,
    tls: Option<config::TlsConfig>,
    app_state: state::AppState<S, T>,
) -> anyhow::Result<()> {
    // create metadata for API docs
    let mut api = aide::openapi::OpenApi {
        info: aide::openapi::Info {
            title: "Keycloak Identity Syncer".to_string(),
            description: Some(format!(
                "Synchronizes users and groups. Source: {}, Target: {}",
                app_state.source.info(),
                app_state.target.read().await.info()
            )),
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

    // Reachable by any client that, when enabled, completed the mTLS handshake.
    let public_routes = aide::axum::ApiRouter::new()
        .api_route(
            "/v1/health",
            aide::axum::routing::get_with(crate::controller::handlers::health::health, crate::controller::handlers::health::health_desc),
        )
        .route("/docs/api.json", aide::axum::routing::get(serve_api))
        .route(
            "/docs",
            aide::redoc::Redoc::new("/docs/api.json").with_title("Keycloak Identity Syncer").axum_route(),
        )
        .route("/", aide::axum::routing::get(|| async { axum::response::Redirect::to("/docs") }));

    // Webhook routes that require `allow_webhook_access` enabled for the certificate when mTLS is enabled.
    let mut protected_routes = aide::axum::ApiRouter::new()
        .api_route(
            "/v1/users",
            aide::axum::routing::get_with(
                crate::controller::handlers::user::list_users,
                crate::controller::handlers::user::list_users_desc,
            ),
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
            "/v1/groups",
            aide::axum::routing::get_with(
                crate::controller::handlers::group::list_groups,
                crate::controller::handlers::group::list_groups_desc,
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
        );

    if tls.as_ref().is_some_and(|tls| tls.client_auth.is_some()) {
        protected_routes = protected_routes.route_layer(axum::middleware::from_fn(tls::require_webhook_access));
    }

    let app = public_routes
        .merge(protected_routes)
        // Spawn each handler as an independent task so it runs to completion even if the client
        // disconnects mid-request. The write lock on AppState::target serializes concurrent
        // handlers with the periodic full sync.
        .route_layer(axum::middleware::from_fn(
            |req: axum::extract::Request, next: axum::middleware::Next| async move { tokio::task::spawn(next.run(req)).await.unwrap() },
        ))
        .with_state(app_state)
        .finish_api(&mut api)
        // Add aide (open API) extension layer
        .layer(axum::Extension(api))
        .layer(sentry::integrations::tower::SentryLayer::new_from_top())
        .layer(sentry::integrations::tower::SentryHttpLayer::new().enable_transaction());

    match tls {
        None => {
            tracing::info!(bind = bind_addr, "Starting API");
            let listener = tokio::net::TcpListener::bind(bind_addr).await?;
            axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
        }
        Some(tls_config) => {
            tls::serve(bind_addr, tls_config, app, shutdown_signal()).await?;
        }
    }

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };

    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received CTRL+C, shutting down");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, shutting down")
        },
    }
}
