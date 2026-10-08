# Webhook CLI

Normally, the KIDS web API is used by the [Keycloak webhook plugin](https://rechenknecht.net/giz/keycloak/keycloak-webhook-spi).
Additionally, this CLI can be used to manually send requests to KIDS.

## Usage

You can execute the CLI directly using `cargo run --bin webhook-cli`, build a binary using `cargo build` or run it via the provided docker image with `docker run dr.rechenknecht.net/giz/keycloak/kids/main/webhook-cli`.

You can get all available global commands and flags from the binary by passing the `--help` option.
There is also a `--help` option available for all commands.
