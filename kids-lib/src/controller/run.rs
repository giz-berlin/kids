use anyhow::Context;

use crate::{
    config,
    controller::{api, state, sync},
};

pub async fn run<S: crate::interface::source::Source + Send + Sync + 'static, T: crate::interface::target::Target + Send + Sync + 'static>(
    api_config: config::Api,
    full_sync_interval_seconds: u64,
    source: S,
    target: T,
) -> anyhow::Result<()> {
    tracing::info!(full_sync_interval = full_sync_interval_seconds, "Starting Controller");

    let app_state = state::AppState {
        source: std::sync::Arc::new(source),
        target: std::sync::Arc::new(tokio::sync::RwLock::new(target)),
    };

    // Start periodic full sync
    let state_for_sync = app_state.clone();
    let periodic_full_sync_handle = tokio::spawn(async move { periodic_full_sync(full_sync_interval_seconds, state_for_sync).await });

    // Run the API or wait for shutdown, but abort immediately if the initial full sync fails.
    // If the initial sync succeeds, the `Ok(Err(e))` pattern does not match and tokio::select!
    // disables that branch, continuing to wait for exit normally (e.g. via Ctrl+C).
    tokio::select! {
        result = async {
            match api_config {
                config::Api::Enabled { bind_addr, tls } => api::run(bind_addr, tls, app_state, shutdown_signal()).await,
                config::Api::Disabled => {
                    tracing::info!("API disabled, running full sync only");
                    shutdown_signal().await;
                    Ok(())
                }
            }
        } => result,
        Ok(Err(e)) = periodic_full_sync_handle => Err(e).context("initial full sync failed on startup"),
    }
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
        _ = ctrl_c => tracing::info!("Received CTRL+C, shutting down"),
        _ = terminate => tracing::info!("Received SIGTERM, shutting down"),
    }
}

pub async fn periodic_full_sync<S: crate::interface::source::Source + Send + Sync + 'static, T: crate::interface::target::Target + Send + Sync + 'static>(
    full_sync_interval_seconds: u64,
    app_state: state::AppState<S, T>,
) -> anyhow::Result<()> {
    if full_sync_interval_seconds == 0 {
        tracing::warn!("full_sync_interval_seconds is set to 0, disabling full sync");
        return Ok(());
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(full_sync_interval_seconds));
    let mut is_initial_sync = true;

    loop {
        // The first tick of the interval completes immediately, so we can perform an initial full
        // sync on startup. This happens in the background task so the API can start accepting connections
        // immediately. Incoming webhook handlers will queue on the target write lock and proceed in order
        // once the sync completes.
        // From the tokio::sync::RwLock docs:
        //
        // Fairness is ensured using a first-in, first-out queue for the tasks awaiting the lock;
        // a read lock will not be given out until all write lock requests that were queued before
        // it have been acquired and released.
        interval.tick().await;

        tracing::info!("Running periodic full sync");

        // Acquire the write lock for the duration of the sync. Incoming webhook handlers wait on
        // the lock and are processed in FIFO order once it is released, so no updates are lost.
        let mut target_guard = app_state.target.write().await;
        if let Err(e) = sync::full_sync(app_state.source.as_ref(), &mut *target_guard).await {
            // If this is the initial full sync return the error so that the application can abort,
            // e.g. because a certain configuration is wrong.
            // If subsequent full syncs fail the error is likely transient (e.g. network outage)
            // so we can simply retry later.
            if is_initial_sync {
                return Err(e);
            }

            tracing::error!(error = ?e, "Periodic full sync failed");
        }

        is_initial_sync = false;
    }
}
