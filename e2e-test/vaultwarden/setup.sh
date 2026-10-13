#!/bin/bash
# Prepares everything that has to exist on the host before `podman compose up`.

set -o errexit
set -o allexport
set -o nounset

cd "$(dirname "$(realpath "$0")")"
source .env

progress_msg() {
  # Color in blue
  printf "\033[0;34m# %s \033[0m\n" "$1"
}

# Vaultwarden has to be served over HTTPS, as the bw CLI used by kids refuses http:// URLs. Kids does not verify the
# certificate (insecure_disable_tls_verification), so a self-signed one is sufficient. Existing files are kept.
generate_cert() {
  if [ -f "certs/$VAULTWARDEN_CONTAINER_NAME.crt" ]; then
    return
  fi

  progress_msg "Generating self-signed certificate for $VAULTWARDEN_CONTAINER_NAME and hostname $PODMAN_SERVICE_HOSTNAME"
  mkdir -p certs
  openssl req -x509 -newkey rsa:2048 -nodes -days "$CERTIFICATE_VALIDITY" \
    -keyout "certs/$VAULTWARDEN_CONTAINER_NAME.key" -out "certs/$VAULTWARDEN_CONTAINER_NAME.crt" \
    -subj "/CN=$PODMAN_SERVICE_HOSTNAME" -addext "subjectAltName=DNS:$PODMAN_SERVICE_HOSTNAME"
}

# Usage: render_template <template> <output> <variable name>...
# Substitutes only the listed variables and fails if one of them is not set.
render_template() {
  TEMPLATE=$1
  OUTPUT=$2
  shift 2
  for VARIABLE in "$@"; do
    : "${!VARIABLE:?$VARIABLE is not set, check .env}"
  done
  envsubst "$(printf '${%s} ' "$@")" < "$TEMPLATE" > "$OUTPUT"
}

# Checks the webhook plugin and renders the realm template, both mounted into the keycloak container.
prepare_keycloak() {
  if [ ! -f ../keycloak/keycloak-webhook-spi-2.0.1.jar ]; then
    echo "Download the keycloak-webhook-spi jar from the latest pipeline in https://rechenknecht.net/giz/keycloak/keycloak-webhook-spi and place it in ../keycloak!"
    echo "Note: In case the version of the plugin increased from 2.0.1, make sure that nothing broke in the update and change the version number in this file, in compose.yml and in ../keycloak/.gitignore."
    exit 1
  fi

  progress_msg "Rendering keycloak realm config"
  render_template config/keycloak_realm_giz.tpl.json config/keycloak_realm_giz.json KEYCLOAK_REALM
}

# Vaultwarden accounts and organizations need client-side encryption to be created, so we start from a
# database with the syncer account and organization already in it (see the VAULTWARDEN_* values in .env).
seed_vaultwarden() {
  if [ -f vw_data/db.sqlite3 ]; then
    return
  fi

  if [ ! -f seed/db.sqlite3 ]; then
    progress_msg "WARN: seed/db.sqlite3 not found, starting with an empty vaultwarden database (see README.md to recreate the seed)"
    return
  fi

  progress_msg "Seeding vaultwarden database"
  mkdir -p vw_data
  cp seed/db.sqlite3 vw_data/db.sqlite3
}

render_kids_config() {
  progress_msg "Rendering kids config"
  render_template config/kids_config.tpl.toml config/kids_config.toml \
    PODMAN_SERVICE_HOSTNAME KEYCLOAK_PORT KEYCLOAK_REALM VAULTWARDEN_TLS_PORT VAULTWARDEN_ORG_ID \
    VAULTWARDEN_USER_CLIENT_ID VAULTWARDEN_USER_CLIENT_SECRET VAULTWARDEN_SYNCER_MASTER_PASSWORD \
    VAULTWARDEN_ORG_CLIENT_ID VAULTWARDEN_ORG_CLIENT_SECRET VAULTWARDEN_ADMIN_TOKEN
}

# Usage: wait_for_http <service name> <url> [curl args...]; retries for up to 5 minutes until the url answers successfully.
wait_for_http() {
  SERVICE_NAME=$1
  URL=$2
  shift 2
  progress_msg "Awaiting $SERVICE_NAME to be healthy..."
  for ATTEMPT in $(seq 1 60); do
    if curl --insecure --head -fsS "$URL" "$@"; then
      progress_msg "OK - $SERVICE_NAME has started"
      return
    fi
    echo "--> Not yet healthy ($ATTEMPT/60)"
    sleep 5
  done
  progress_msg "ERROR: $SERVICE_NAME did not become healthy in time"
  exit 1
}

wait_for_services() {
  wait_for_http "$KEYCLOAK_CONTAINER_NAME" "http://$PODMAN_SERVICE_HOSTNAME:$KEYCLOAK_MANAGEMENT_PORT/health/ready"
  wait_for_http "$VAULTWARDEN_CONTAINER_NAME" "https://$PODMAN_SERVICE_HOSTNAME:$VAULTWARDEN_TLS_PORT/alive"
}

progress_msg "Checking if /etc/hosts file is correctly setup"
if ! grep -q "^127.0.0.1 $PODMAN_SERVICE_HOSTNAME" /etc/hosts; then
  echo "a line '127.0.0.1 $PODMAN_SERVICE_HOSTNAME' must be contained in /etc/hosts file!"
  exit 1
fi

generate_cert
prepare_keycloak
seed_vaultwarden
render_kids_config

progress_msg "Starting services"
podman compose up -d
wait_for_services
