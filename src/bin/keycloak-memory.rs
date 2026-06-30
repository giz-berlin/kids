use kids::{cli, source, target};

/// The keycloak-memory binary uses the in-memory target to provide an easy-to-use
/// target for local and E2E testing of core functionality without having to spin
/// up an entire external service.
fn main() -> anyhow::Result<()> {
    cli::run::<source::keycloak::Connector, target::memory::Connector>()
}
