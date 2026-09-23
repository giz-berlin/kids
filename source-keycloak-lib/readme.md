# Keycloak Source

This file describes the setup on the Keycloak side.
For the configuration of KIDS, have a look at the [example config file](../default_configs/keycloak-source.config.example.toml).

## Keycloak Webhook SPI Provider

Install the [Keycloak Webhook SPI provider](https://rechenknecht.net/giz/keycloak/keycloak-webhook-spi).
Information about the webhook configuration can be found in the provider's repository.
You need to set at least the following values for a KIDS webhook:

- `name`: Your decision.
- `url`: `https://<KIDS' IP / domain>:<port>`
- `authToken`: Not needed.
- `realm`: The Keycloak realm with your users and clients for the email server.
- `version`: `v1`
- `serverCertificate`: The base64-encoded value of `controller.api.tls.cert_pem` in your KIDS config. Can be generated with the [provided certificate generation script](../scripts/generate-cert.sh).
- `clientCertificate` and `clientKey`: Base64-encoded certificate and key for Keycloak.
  The values can be generated with the [provided certificate generation script](../scripts/generate-cert.sh).
  Make sure to add a client with the same certificate (not base64-encoded) and the role `source` to `controller.api.tls.client_auth.clients` in your KIDS config.

## Keycloak Client

- Enable client authentication.
- Only enable the `Client Credentials Grant` authentication flow.
- Leave all URLs blank as Keycloak does not redirect to the client.
- Disable `Full Scope allowed` in the dedicated client scope.
- Add the following client roles to the dedicated client scope:
  - `view-users` (client role of the `realm-management` client)
  - `view-clients` (client role of the `realm-management` client)

## Keycloak Service Account

When creating the client with the `Client Credentials Grant`, Keycloak automatically creates a service account.
You can edit the roles of the service account in the `Service account roles` tab in the client configuration.
You have to add the following roles:

- `view-users` (client role of the `realm-management` client)
- `view-clients` (client role of the `realm-management` client)
