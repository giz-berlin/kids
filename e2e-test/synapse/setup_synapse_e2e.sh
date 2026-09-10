#!/bin/bash

set -o errexit
set -o allexport
set -o nounset

# By default, podman includes the /etc/hosts file of the host system in the /etc/hosts of the
# containers. We have to disable that behavior, because we have already modified the host system file so that the service hostnames
# from the perspectives of the containers and the host can match:
#    host /etc/hosts: 127.0.0.1 host.docker.internal[=$PODMAN_SERVICE_HOSTNAME]
#    container /etc/hosts: <host ip> host.docker.internal   [will be automatically inserted by podman, but ONLY if DNS name
#                                                            not already contained in file previously]
# See https://github.com/containers/common/blob/main/docs/containers.conf.5.md
export CONTAINERS_CONF=$(realpath ./config/containers.conf)

progress_msg() {
  # Color in blue
  printf "\033[0;34m# %s \033[0m\n" "$1"
}

source .env

sign() {
    SERVICE_NAME=$1
    HOST_NAME=$2
    EXTFILE="$SERVICE_NAME.ext"
    cat > "$EXTFILE" <<EOF
authorityKeyIdentifier=keyid,issuer
basicConstraints=CA:FALSE
keyUsage = digitalSignature, nonRepudiation, keyEncipherment, dataEncipherment
subjectAltName = @alt_names

[alt_names]
DNS.1 = $HOST_NAME
EOF

    progress_msg "Signing certificate for $SERVICE_NAME and hostname $HOST_NAME using local CA"
    openssl x509 -req -in "$SERVICE_NAME.csr" -CA "$KIDS_CA_NAME.crt" -CAkey "$KIDS_CA_NAME.key" \
        -CAcreateserial -out "$SERVICE_NAME.crt" -days $CERTIFICATE_VALIDITY -sha256 -extfile "$EXTFILE"

    progress_msg "WARN: Making $SERVICE_NAME.key world-readable"
    chmod 644 $SERVICE_NAME.key
}

restart_if_possible() {
  CONTAINER_NAME=$1
  # Inspecting the container works iff it exists
  if podman container inspect $CONTAINER_NAME >/dev/null; then
      progress_msg "(Re)starting $CONTAINER_NAME"
      # Note: Not using restart here, because this sometimes fails to bind the exposed ports (which appear to be still
      # in use by the very container *itself*)...
      podman stop $CONTAINER_NAME
      podman start $CONTAINER_NAME
      return 0
  fi

  return 1
}

progress_msg "Checking if /etc/hosts file is correctly setup"
if grep "^127.0.0.1 $PODMAN_SERVICE_HOSTNAME" /etc/hosts; [ $? -ne 0 ]; then
    echo "a line '127.0.0.1 $PODMAN_SERVICE_HOSTNAME' must be contained in /etc/hosts file!"
    exit 1
else
    echo "OK"
fi

if [ ! -d local_ca ]; then
  progress_msg "Generating local CA with name $KIDS_CA_NAME".
  mkdir local_ca
  cd local_ca
  openssl genrsa -out "$KIDS_CA_NAME.key" 4096
  openssl req -x509 -new -nodes -key "$KIDS_CA_NAME.key" -sha256 -days $CERTIFICATE_VALIDITY -out "$KIDS_CA_NAME.crt" \
     -subj "/C=US/ST=Local/L=Local/O=MyOrg/OU=Dev/CN=KIDS Local CA"
  cd ..
fi

if restart_if_possible $REVERSE_PROXY_CONTAINER_NAME; [ $? -ne 0 ]; then
  progress_msg "Starting $REVERSE_PROXY_CONTAINER_NAME"
  envsubst < config/synapse_admin.tpl.Caddyfile > config/synapse_admin.Caddyfile
  mkdir -p ./caddy_data
  mkdir -p ./caddy_config
  podman run -d --name "$REVERSE_PROXY_CONTAINER_NAME" \
    -p "$SYNAPSE_ADMIN_TLS_PORT:443" \
    -p "$MAS_TLS_PORT:444" \
    -p "$SYNAPSE_TLS_PORT:445" \
    -v "./local_ca/$KIDS_CA_NAME.crt:/certs/root.pem" \
    -v "./local_ca/$KIDS_CA_NAME.key:/certs/root.key" \
    -v "./config/synapse_admin.Caddyfile:/etc/caddy/Caddyfile" \
    -v "./caddy_data:/data" \
    -v "./caddy_config:/config" \
    docker.io/caddy
fi

if restart_if_possible $KEYCLOAK_CONTAINER_NAME; [ $? -ne 0 ]; then
  cd local_ca
  progress_msg "Generating certificate for $KEYCLOAK_CONTAINER_NAME"
  openssl genrsa -out "$KEYCLOAK_CONTAINER_NAME.key" 2048
  openssl req -new -key "$KEYCLOAK_CONTAINER_NAME.key" -out "$KEYCLOAK_CONTAINER_NAME.csr" \
      -subj "/C=US/ST=Local/L=Local/O=MyOrg/OU=Dev/CN=$PODMAN_SERVICE_HOSTNAME"
  sign $KEYCLOAK_CONTAINER_NAME $PODMAN_SERVICE_HOSTNAME

  cd ..

  if [ ! -f ../keycloak/keycloak-webhook-spi-2.0.0.jar ]; then
    echo "Download the keycloak-webhook-spi jar from the latest pipeline in https://rechenknecht.net/giz/keycloak/keycloak-webhook-spi and place it in ../keycloak!"
    echo "Note: In case the version of the plugin increased from 2.0.0, make sure that nothing broke in the update and change the version number in this file and in ../keycloak/.gitignore."
    exit 1
  fi

  progress_msg "Starting $KEYCLOAK_CONTAINER_NAME podman container with hostname $PODMAN_SERVICE_HOSTNAME"
  envsubst < config/keycloak_realm_giz.tpl.json > config/keycloak_realm_giz.json
  podman run \
      -d --name $KEYCLOAK_CONTAINER_NAME \
      -e KC_BOOTSTRAP_ADMIN_USERNAME=admin -e KC_BOOTSTRAP_ADMIN_PASSWORD=password \
      -e KC_HEALTH_ENABLED=true -e KC_HOSTNAME_STRICT=false \
      -e KC_HTTPS_CERTIFICATE_FILE=/opt/keycloak/ca/$KEYCLOAK_CONTAINER_NAME.crt \
      -e KC_HTTPS_CERTIFICATE_KEY_FILE=/opt/keycloak/ca/$KEYCLOAK_CONTAINER_NAME.key \
      -v "./local_ca:/opt/keycloak/ca" -v "./config/keycloak_realm_giz.json:/opt/keycloak/data/import/keycloak_realm_giz.json" \
      -v "../keycloak/keycloak-webhook-spi-2.0.0.jar:/opt/keycloak/providers/keycloak-webhook-spi-2.0.0.jar" \
      -v "../keycloak/webhook-config.json:/opt/keycloak/conf/webhook-config.json" \
      -p "0.0.0.0:8443:8443" -p "127.0.0.1:9000:9000" \
      quay.io/keycloak/keycloak:26.2 start --import-realm
fi

progress_msg "Awaiting $KEYCLOAK_CONTAINER_NAME to be healthy..."
until curl --insecure --head -fsS https://$PODMAN_SERVICE_HOSTNAME:9000/health/ready --http1.1
do
    echo "--> Not yet healthy"
    sleep 5;
done
progress_msg "OK - $KEYCLOAK_CONTAINER_NAME has started"

SHOULD_CREATE_USERS=0
if restart_if_possible $SYNAPSE_CONTAINER_NAME; [ $? -ne 0 ]; then
  export SYNAPSE_HOSTNAME=$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT

  cd local_ca
  progress_msg "Generating certificate for $SYNAPSE_CONTAINER_NAME"
  openssl genrsa -out "$SYNAPSE_CONTAINER_NAME.key" 2048
  openssl req -new -key "$SYNAPSE_CONTAINER_NAME.key" -out "$SYNAPSE_CONTAINER_NAME.csr" \
      -subj "/C=US/ST=Local/L=Local/O=MyOrg/OU=Dev/CN=$SYNAPSE_HOSTNAME"
  sign $SYNAPSE_CONTAINER_NAME $SYNAPSE_HOSTNAME

  cd ..

  progress_msg "Generating configuration file for $SYNAPSE_CONTAINER_NAME"
  mkdir -p synapse_data
  # Ensure config file is owned by root in container/our user on the host machine so that we can edit it without sudo
  podman run -it --rm \
      -e UID=0 -e GID=0 \
      -v "./synapse_data:/data" \
      -e SYNAPSE_SERVER_NAME=$SYNAPSE_HOSTNAME \
      -e SYNAPSE_REPORT_STATS=no \
      docker.io/matrixdotorg/synapse:latest generate

  progress_msg "Adjusting $SYNAPSE_CONTAINER_NAME config"
  python3 modify_synapse_config.py

  progress_msg "Starting $SYNAPSE_CONTAINER_NAME podman container with hostname $SYNAPSE_HOSTNAME"
  podman run \
      -d --name $SYNAPSE_CONTAINER_NAME \
      -e UID=0 -e GID=0 \
      -e SSL_CERT_FILE=/opt/ca/$KIDS_CA_NAME.crt \
      -v "./local_ca:/opt/ca" -v "./synapse_data:/data" \
      -p "0.0.0.0:$SYNAPSE_PORT:$SYNAPSE_PORT" \
      docker.io/matrixdotorg/synapse:latest

  export SHOULD_CREATE_USERS=1
fi

if restart_if_possible $MAS_POSTGRES_CONTAINER_NAME; [ $? -ne 0 ]; then
  export MAS_POSTGRES_HOSTNAME=$PODMAN_SERVICE_HOSTNAME:$MAS_POSTGRES_PORT

  progress_msg "Starting $MAS_POSTGRES_CONTAINER_NAME podman container with hostname $MAS_POSTGRES_HOSTNAME"
  podman run \
      -d --name $MAS_POSTGRES_CONTAINER_NAME \
      -e POSTGRES_DB=$MAS_POSTGRES_DATABASE \
      -e POSTGRES_USER=$MAS_POSTGRES_USER \
      -e POSTGRES_PASSWORD=$MAS_POSTGRES_PASSWORD \
      -p "0.0.0.0:$MAS_POSTGRES_PORT:5432" \
      docker.io/postgres:18
fi
progress_msg "Awaiting $MAS_POSTGRES_CONTAINER_NAME to be healthy..."
until podman exec "$MAS_POSTGRES_CONTAINER_NAME" pg_isready
do
    echo "--> Not yet healthy"
    sleep 1;
done
progress_msg "OK - $MAS_POSTGRES_CONTAINER_NAME has started"

if restart_if_possible $MAS_CONTAINER_NAME; [ $? -ne 0 ]; then
  export MAS_HOSTNAME=$PODMAN_SERVICE_HOSTNAME:$MAS_PORT

  progress_msg "Generating configuration file for $MAS_CONTAINER_NAME"
  mkdir -p synapse_data
  podman run --rm \
      ghcr.io/element-hq/matrix-authentication-service:latest config generate > synapse_data/mas-config.yaml

  progress_msg "Adjusting $MAS_CONTAINER_NAME config"
  python3 modify_mas_config.py
  podman run --rm \
      -v "./synapse_data/mas-config.yaml:/config.yaml" \
      ghcr.io/element-hq/matrix-authentication-service:latest config check --config /config.yaml

  podman run -d --name "$MAS_CONTAINER_NAME" \
      -v "./synapse_data/mas-config.yaml:/config.yaml" \
      -v "./local_ca/$KIDS_CA_NAME.crt:/etc/ssl/certs/ca-certificates.crt" \
      -p "0.0.0.0:$MAS_PORT:8080" \
      ghcr.io/element-hq/matrix-authentication-service:latest server --config /config.yaml
fi

progress_msg "Awaiting $SYNAPSE_CONTAINER_NAME to be healthy..."
until curl --insecure --head -fsS https://$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT/health
do
    echo "--> Not yet healthy"
    sleep 5;
done
until curl --head -fsS http://$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_PORT/health
do
    echo "--> Not yet healthy"
    sleep 5;
done
progress_msg "OK - $SYNAPSE_CONTAINER_NAME has started"

progress_msg "Awaiting $MAS_CONTAINER_NAME to be healthy..."
until curl --head -fsS http://$PODMAN_SERVICE_HOSTNAME:$MAS_PORT/health --http1.1
do
    echo "--> Not yet healthy"
    sleep 1;
done
podman exec -it kids-e2e-mas mas-cli doctor --config /config.yaml
progress_msg "OK - $MAS_CONTAINER_NAME has started"

if [ $SHOULD_CREATE_USERS -eq 1 ]; then
  progress_msg "Creating admin user in $SYNAPSE_CONTAINER_NAME"
  podman exec -it "$MAS_CONTAINER_NAME" mas-cli manage register-user --config /config.yaml --yes --ignore-password-complexity --password password --admin admin
  ADMIN_ACCESS_TOKEN="$(podman exec -it "$MAS_CONTAINER_NAME" mas-cli manage issue-compatibility-token --config /config.yaml --yes-i-want-to-grant-synapse-admin-privileges admin | cut -d " " -f 8)"
  echo "ADMIN_ACCESS_TOKEN=$ADMIN_ACCESS_TOKEN" > ./.env.token
fi

if restart_if_possible $SYNAPSE_ADMIN_CONTAINER_NAME; [ $? -ne 0 ]; then
  progress_msg "Starting $SYNAPSE_ADMIN_CONTAINER_NAME"
  envsubst < config/synapse_admin_config.tpl.json > config/synapse_admin_config.json
  podman run -d --name $SYNAPSE_ADMIN_CONTAINER_NAME -p "$SYNAPSE_ADMIN_PORT:8080" -v "./config/synapse_admin_config.json:/app/config.json" ghcr.io/etkecc/ketesa
fi

progress_msg "Creating synapse_e2e_config.toml from environment variables"
envsubst < config/synapse_e2e_config.tpl.toml > config/synapse_e2e_config.toml
