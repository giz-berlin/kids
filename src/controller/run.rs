use anyhow::Context;

use crate::{
    controller::{api, state, sync},
    source, target,
};

pub async fn run<S: source::interface::Source + Send + Sync + 'static, T: target::interface::Target + Send + Sync + 'static>(
    bind_addr: std::net::SocketAddr,
    tls: crate::config::Tls,
    full_sync_interval_seconds: u64,
    source: S,
    target: T,
) -> anyhow::Result<()> {
    tracing::info!(addr = %bind_addr, full_sync_interval = full_sync_interval_seconds, "Starting Controller");

    let app_state = state::AppState {
        source: std::sync::Arc::new(source),
        target: std::sync::Arc::new(tokio::sync::RwLock::new(target)),
    };

    // Start periodic full sync
    let state_for_sync = app_state.clone();
    let periodic_full_sync_handle = tokio::spawn(async move { periodic_full_sync(full_sync_interval_seconds, state_for_sync).await });

    // Run the API, but abort immediately if the initial full sync fails.
    // If the initial sync succeeds, the `Ok(Err(e))` pattern does not match and tokio::select!
    // disables that branch, continuing to wait for the API to exit normally (e.g. via Ctrl+C).
    tokio::select! {
        result = api::run(bind_addr, tls, app_state) => result,
        Ok(Err(e)) = periodic_full_sync_handle => Err(e).context("initial full sync failed on startup"),
    }
}

pub async fn periodic_full_sync<S: source::interface::Source + Send + Sync + 'static, T: target::interface::Target + Send + Sync + 'static>(
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
