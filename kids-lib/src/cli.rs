use std::str::FromStr;

use anyhow::Context;
use clap::{ArgMatches, Command};
use tracing_subscriber::Layer;

/// KIDS - Keycloak Identity Syncer
#[derive(Debug)]
pub struct CliArguments {
    pub config: std::path::PathBuf,
}

/// Build a default clap [`Command`].
/// Pass all string arguments as `env!("CARGO_PKG_*")` from your binary crate.
pub fn parse_command(
    name: &'static str,
    version: &'static str,
    author: &'static str,
    about: &'static str,
    long_about: &'static str,
    homepage: &'static str,
) -> Command {
    Command::new(name)
        .version(version)
        .author(author)
        .about(about)
        .long_about(long_about)
        .after_help(homepage)
        .arg(
            clap::Arg::new("config")
                .long("config")
                .short('c')
                .default_value("config.toml")
                .value_name("FILE")
                .help("Path to the config file"),
        )
}

impl CliArguments {
    /// Parse [`CliArguments`] from clap [`ArgMatches`].
    pub fn from_arg_matches(matches: &ArgMatches) -> anyhow::Result<Self> {
        let config = matches
            .get_one::<String>("config")
            .map(std::path::PathBuf::from)
            .context("failed to parse config path argument")?;

        Ok(CliArguments { config })
    }
}

/// Run the sync engine with pre-parsed CLI arguments.
///
/// Most binary crates should use the [`cli_run!`] macro instead, which handles
/// argument parsing using that crate's own `CARGO_PKG_*` environment variables.
pub fn run_with_args<S: crate::interface::source::Source + Send + Sync + 'static, T: crate::interface::target::Target + Send + Sync + 'static>(
    args: CliArguments,
) -> anyhow::Result<()> {
    let config = crate::config::Config::<S::Config, T::Config>::try_from(args.config)?;

    // Install default CryptoProvider for Rustls crate features.
    // Without this, the program panicks.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let _guard = if let Some(sentry_config) = config.sentry.as_ref() {
        let dsn = sentry::types::Dsn::from_str(&sentry_config.dsn).map_err(|err| {
            tracing::error!(error = ?err, "Invalid Sentry DSN {}", &sentry_config.dsn);
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
        let target_impl = match T::new(config.target).await {
            Ok(target_impl) => target_impl,
            Err(e) => {
                panic!("{}", e)
            }
        };

        tracing::info!("Active Source: {}", source_impl.info());
        tracing::info!("Active Target: {}", target_impl.info());

        crate::controller::run(config.controller.api, config.controller.full_sync_interval_seconds, source_impl, target_impl)
            .await
            .unwrap();
    });

    Ok(())
}

/// Convenience macro that parses CLI arguments using the *calling crate's*
/// `CARGO_PKG_NAME`, `CARGO_PKG_VERSION`, `CARGO_PKG_AUTHORS`,
/// `CARGO_PKG_DESCRIPTION` and `CARGO_PKG_HOMEPAGE` environment variables,
/// then runs the sync loop with the given source and target types.
///
/// # Example
/// ```ignore
/// fn main() -> anyhow::Result<()> {
///     kids_lib::cli_run!(source_keycloak_lib::Connector, target::Connector)
/// }
/// ```
#[macro_export]
macro_rules! cli_run {
    ($source:path, $target:path) => {{
        fn _run_inner() -> std::result::Result<(), anyhow::Error> {
            let command = $crate::cli::parse_command(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
                env!("CARGO_PKG_AUTHORS"),
                concat!("KIDS - Keycloak Identity Syncer: ", env!("CARGO_PKG_NAME")),
                env!("CARGO_PKG_DESCRIPTION"),
                concat!("See also the project website at ", env!("CARGO_PKG_HOMEPAGE")),
            );
            let matches = command.get_matches();
            let args = $crate::cli::CliArguments::from_arg_matches(&matches)?;
            $crate::cli::run_with_args::<$source, $target>(args)
        }
        _run_inner()
    }};
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
