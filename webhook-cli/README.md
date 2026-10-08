# Webhook CLI

Normally, the KIDS web API is used by the [Keycloak webhook plugin](https://rechenknecht.net/giz/keycloak/keycloak-webhook-spi).
Additionally, this CLI can be used to manually send requests to KIDS.

## Usage

You can execute the CLI directly using `cargo run`, build a binary using `cargo build` or run it via the provided docker image with `docker run dr.rechenknecht.net/giz/keycloak/kids/main/webhook-cli`.

You can get all available commands and flags from the binary by executing the `help` command.
There is also a `help` subcommand available for all commands.

## Configuration

All necessary config options are provided via CLI flags.
They relate directly to your KIDS configuration:

- `--insecure`: Use this if you set `controller.api.tls.mode` to `insecure_disabled`.
- `--endpoint`: HTTP URL for the configured `controller.api.bind_addr`.
- `--server-ca`: The certificate of the KIDS API configured with `controller.api.tls.cert_pem`.
- `--client-key` and `--client-cert`: Key and certificate for the client configured in `controller.api.tls.client_auth.clients`.
