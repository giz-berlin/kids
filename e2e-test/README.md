# (Manual) End-to-End Testing

This directory contains utility scripts to easily setup and teardown services we want to sync to/from in this project 
for the purpose of local testing:

## Setup

You will need to install [podman](https://docs.podman.io/en/latest/index.html), as the scripts make use of (rootless)
podman to start containers.

### macOS

To use podman with macOS, you need to create a virtual machine first:

```podman machine init -v /host/system/path/kids/e2e-test:/path/in/machine/e2e-test```

Mounting a folder into the virtual machine is required so that the scripts can also mount it into the container. 

Start the machine with `podman machine start`. You should now be able to use podman commands normally.

## Scripts

The [setup_synapse_e2e.sh](setup_synapse_e2e.sh) script will spin up [keycloak](https://www.keycloak.org/server/containers) 
and [synapse](https://hub.docker.com/r/matrixdotorg/synapse) containers, as well as a
[synapse admin](https://github.com/etkecc/synapse-admin) interface.
Also, it will automatically setup a local CA, because Synapse expects OIDC providers to use HTTPS.
The [teardown_synapse_e2e.sh](teardown_synapse_e2e.sh) script will delete any traces of these services.

### Using the Synapse Admin Web Interface

The Synapse Admin web interface needs to connect to the Synapse API, which is likely to fail initially because the
browser will reject the certificate of Synapse. In order to fix that, you may need to access the Synapse API manually
once (located under `https://{PODMAN_SERVICE_HOSTNAME}:{SYNAPSE_TLS_PORT}`, see [.env file](.env)) and
dismiss the certificate warning presented to you. This should make the browser trust the self-signed
certificate going forward, which will enable Synapse Admin to work properly.

## Manually (re)starting containers

Before running any commands with podman manually, make sure to run `export CONTAINERS_CONF=./containers.conf` in
your terminal. See [here](setup_synapse_e2e.sh) for an explanation (tl;dr: networking might not work as expected 
with your manually (re)started container if you don't do it).
