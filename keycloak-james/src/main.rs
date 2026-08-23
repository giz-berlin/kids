mod target;

fn main() -> anyhow::Result<()> {
    kids_lib::cli::run::<source_keycloak_lib::Connector, target::Connector>()
}
