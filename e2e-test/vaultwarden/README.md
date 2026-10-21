# Vaultwarden E2E Test Setup

This directory contains utility scripts to easily setup and teardown services we want to sync to/from in this project
for the purpose of local testing.

## Setup

Check the [Synapse e2e Readme](../synapse/README.md) for the base setup, as we use a similar stack. We additionally use `podman-compose` here.

## Scripts

The [setup.sh](setup.sh) script will spin up [Keycloak](https://www.keycloak.org/server/containers)
and [Vaultwarden](https://github.com/dani-garcia/vaultwarden) containers.
Additionally, it will generate a self-signed certificate for Vaultwarden. Vaultwarden has to be served over HTTPS because the `bw` CLI used by KIDS refuses `http://` URLs. KIDS does not verify the certificate, so a self-signed one is sufficient; browsers will show a warning.
The [teardown.sh](teardown.sh) script will delete any traces of these services.

You can now access (see [.env](.env) for environment variables)

- Keycloak at `http://$PODMAN_SERVICE_HOSTNAME:8080`
- Vaultwarden at `https://$PODMAN_SERVICE_HOSTNAME:8000`

### Configuration

The setup script will create a `config/kids_config.toml` file that can be used as the `config.toml` for KIDS. This should enable running the `keycloak-vaultwarden` KIDS binary out of the box, but adjust the config to your liking.

## The Vaultwarden Server

We currently use [OIDCWarden](https://github.com/Timshel/OIDCWarden) instead of the upstream Vaultwarden, because that doesn't support all features we use. Because of this, the e2e test runs against OIDCWarden by default. The server image is set with `VAULTWARDEN_IMAGE` in `.env`. To test against another compatible server, for example upstream Vaultwarden, change it to e.g. `docker.io/vaultwarden/server:<tag>`, then run `./teardown.sh && ./setup.sh`. Always use a pinned tag, as the target depends on the server's API.

The seed database was created with upstream Vaultwarden and works with both servers.

## The Seed Database (`seed/db.sqlite3`)

Creating a Vaultwarden account or organization requires client-side encryption (master key derivation, key pairs, ...), which only the official clients implement. Instead of re-implementing that, the e2e setup starts from a committed database that already contains:

- the syncer account (`VAULTWARDEN_SYNCER_EMAIL` / `VAULTWARDEN_SYNCER_MASTER_PASSWORD`)
- the organization `Test Org`, in which only the syncer is a member
- a second, already registered account that is **not** a member of the organization: `user@test.giz.berlin` ("Second Tester", master password `another_secure_master_password`). Its email matches the keycloak test user `user@test.giz.berlin`, so the verification can check that kids adds and confirms an _existing_ account, as opposed to `tester@test.giz.berlin`, which has no account yet and stays invited.

`setup.sh` copies it to `vw_data/db.sqlite3` if no database exists yet. `teardown.sh` removes `vw_data/`, so every fresh setup starts from the seed again.

The matching IDs and API keys live in `.env` (`VAULTWARDEN_USER_CLIENT_*`, `VAULTWARDEN_ORG_*`) and are rendered into `config/kids_config.toml`. **The seed and these values must always be changed together.**

### How to recreate it

1. Start from an empty database:

   ```sh
   ./teardown.sh
   rm seed/db.sqlite3
   ./setup.sh
   ```

2. Open `https://host.docker.internal:8000` in a browser and create the syncer account with the email and master password you want to use.
3. Log in and create the organization.
   Then register the second account (`user@test.giz.berlin`) as well, but do **not** invite it to the organization.
4. Collect the credentials from the web vault:
   - user API key: _Account settings_ → _Security_ → _Keys_ → _View API key_ (client ID `user.<id>` and secret)
   - organization ID: from the URL of the organization's page
   - organization API key: organization _Settings_ → _Organization info_ → _API key_ (client ID `organization.<id>` and secret)
5. Update the `VAULTWARDEN_SYNCER_*`, `VAULTWARDEN_USER_CLIENT_*` and `VAULTWARDEN_ORG_*` values in `.env`.
6. Snapshot the database, then drop the login sessions of the browser:

   ```sh
   sqlite3 vw_data/db.sqlite3 ".backup seed/db.sqlite3"
   sqlite3 seed/db.sqlite3 "delete from devices; vacuum;"
   ```

7. Verify with a fresh start: `./teardown.sh && ./setup.sh`, then check that the new API keys work and commit `seed/db.sqlite3` together with `.env`.
