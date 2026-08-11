# (Manual) End-to-End Testing

This directory contains utility scripts to easily setup and teardown services we want to sync to/from in this project 
for the purpose of local testing:

## Setup

You will need to install [podman](https://docs.podman.io/en/latest/index.html), as the scripts make use of (rootless)
podman to start containers.

### macOS

To use podman with macOS, you need to create a virtual machine first:

```shell
podman machine init -v /host/system/path/kids/e2e-test:/path/in/machine/e2e-test
```

Mounting a folder into the virtual machine is required so that the scripts can also mount it into the container. 

Start the machine with `podman machine start`. You should now be able to use podman commands normally.

## Scripts

The [setup_synapse_e2e.sh](setup_synapse_e2e.sh) script will spin up [Keycloak](https://www.keycloak.org/server/containers) 
and [Synapse](https://hub.docker.com/r/matrixdotorg/synapse) containers, as well as a
[Synapse Admin](https://github.com/etkecc/synapse-admin) interface.
Additionally, it will automatically setup a local CA, because Synapse expects OIDC providers to use HTTPS.
The [teardown_synapse_e2e.sh](teardown_synapse_e2e.sh) script will delete any traces of these services.

You can now access (see [.env](.env) for environment variables)

- Keycloak at `https://$PODMAN_SERVICE_HOSTNAME:8443`
- Synapse Admin at `http://$PODMAN_SERVICE_HOSTNAME:8080`
- Synapse at `http://$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_PORT` and `https://$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT`

### Configuration

The setup script will create a `config/synapse_e2e_config.toml` file that can be used as the `config.toml` for KIDS. This should enable running the `keycloak-synapse` KIDS binary out of the box, but adjust the config to your liking.

## Manually (re)starting containers

Before running any commands with podman manually, make sure to run `export CONTAINERS_CONF=./containers.conf` in
your terminal. See [here](setup_synapse_e2e.sh) for an explanation (tl;dr: networking might not work as expected 
with your manually (re)started container if you don't do it).
