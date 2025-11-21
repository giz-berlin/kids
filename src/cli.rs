use std::str::FromStr;

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::Layer;

use crate::types::SharedResourceIdentifier;
use crate::{config, source, target};

#[derive(clap::Parser, Debug)]
#[command(author, version, about, long_about = "KIDS - Keycloak Identity Syncer")]
pub struct CliArguments {
    #[clap(long, short, default_value = "config.toml", help = "Path to the config file")]
    config: std::path::PathBuf,
}

pub fn run<S: source::interface::Source + Send + Sync + 'static, T: target::interface::Target + Send + Sync + 'static>() -> anyhow::Result<()> {
    let args = CliArguments::parse();

    let config = config::Config::<S::Config, T::Config>::try_from(args.config)?;

    let _guard = if let Some(sentry_config) = config.sentry.as_ref() {
        let dsn = sentry::types::Dsn::from_str(&sentry_config.dsn).map_err(|err| {
            tracing::error!("Invalid Sentry DSN {}: {}", &sentry_config.dsn, err);
            err
        })?;

        let guard = sentry::init(sentry::ClientOptions {
            dsn: Some(dsn),
            release: sentry::release_name!(),
            environment: Some(sentry_config.environment.clone().into()),
            attach_stacktrace: true,
            trim_backtraces: true,
            // TODO: We may not want to have all transactions and thus set this to a lower value.
            // See https://docs.sentry.io/platforms/rust/tracing/
            traces_sample_rate: 1.0,
            in_app_include: vec!["kids"],
            ..Default::default()
        });

        Some(guard)
    } else {
        None
    };

    init_logging().context("initializing logging")?;

    // Sentry should not be initialised inside a tokio async function, thus this weird workaround.
    // See https://docs.sentry.io/platforms/rust/#async-main-function.
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap().block_on(async {
        let source_impl = S::new(config.source);
        let mut target_impl = match T::new(config.target).await {
            Ok(target_impl) => target_impl,
            Err(e) => {
                panic!("{}", e)
            }
        };

        tracing::info!("Active Source: {}", source_impl.info());
        tracing::info!("Active Target: {}", target_impl.info());

        // Initialize target mapping.
        // In the future, the controller would perform a proper full-sync on startup.
        target_impl.full_sync_incoming().await.unwrap();

        crate::controller::api::run(config.http.bind_addr, source_impl, target_impl).await.unwrap();
    });

    Ok(())
}

fn init_logging() -> anyhow::Result<()> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    // Consider the standard RUST_LOG environment variable as default.
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        .from_env_lossy();

    let console_log_layer = tracing_subscriber::fmt::layer().pretty().with_target(true).with_filter(filter);

    let sentry_layer = sentry::integrations::tracing::layer()
        .enable_span_attributes()
        .event_filter(|md| match *md.level() {
            tracing::Level::TRACE => sentry::integrations::tracing::EventFilter::Ignore,
            tracing::Level::DEBUG => sentry::integrations::tracing::EventFilter::Breadcrumb,
            tracing::Level::INFO => sentry::integrations::tracing::EventFilter::Breadcrumb,
            tracing::Level::WARN => sentry::integrations::tracing::EventFilter::Event,
            tracing::Level::ERROR => sentry::integrations::tracing::EventFilter::Event,
        });

    tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_log_layer)
        .try_init()
        .context("initializing tracing subscriber")
}
