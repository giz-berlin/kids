use kids::{cli, source, target};

fn main() -> anyhow::Result<()> {
    cli::run::<source::keycloak::Connector, target::synapse::Connector>()
}
