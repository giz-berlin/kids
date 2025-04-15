use kids::{controller, source, target};

fn main() -> anyhow::Result<()> {
    controller::start_controller::<source::keycloak::Connector, target::james::Connector>()
}
