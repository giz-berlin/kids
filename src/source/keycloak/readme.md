# Keycloak Source

This file describes the setup on the Keycloak side.
For the configuration of KIDS, have a look at the [example config file](../../../default_configs/keycloak-source.config.example.toml).

## Keycloak Client

- Enable client authentication.
- Leave all URLs blank as Keycloak does not redirect to the client.
- Only enable the `Client Credentials Grant` authentication flow.
- Enable `Full Scope allowed` in the dedicated client scope.

## Keycloak Service Account

When creating the client, Keycloak automatically creates a service account.
You can edit the roles of the service account in the `Service account roles` tab in the client configuration.
You have to add the following roles:

- `view-users` (client role of the `realm-management` client)
- `view-realm` (client role of the `realm-management` client)
