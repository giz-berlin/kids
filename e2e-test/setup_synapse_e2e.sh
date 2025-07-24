#!/bin/bash

set -e
set -a

# By default, podman includes the /etc/hosts file of the host system in the /etc/hosts of the
# containers. We have to disable that behavior, because we have already modified the host system file so that the service hostnames
# from the perspectives of the containers and the host can match:
#    host /etc/hosts: 127.0.0.1 host.docker.internal
#    container /etc/hosts: <host ip> host.docker.internal   [will be automatically inserted by postman, but ONLY if DNS name
#                                                            not already contained in file previously]
# See https://github.com/containers/common/blob/main/docs/containers.conf.5.md
export CONTAINERS_CONF=./containers.conf

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

    progress_msg "Signing certificate for $SERVICE_NAME and hostname $HOST_NAME using local CA."
    openssl x509 -req -in "$SERVICE_NAME.csr" -CA "$KIDS_CA_NAME.crt" -CAkey "$KIDS_CA_NAME.key" \
        -CAcreateserial -out "$SERVICE_NAME.crt" -days $CERTIFICATE_VALIDITY -sha256 -extfile "$EXTFILE"

    progress_msg "WARN: Making $SERVICE_NAME.key world-readable"
    chmod 644 $SERVICE_NAME.key
}

restart_if_possible() {
  CONTAINER_NAME=$1
  if podman ps -a | grep $CONTAINER_NAME; then
      progress_msg "(Re)starting $CONTAINER_NAME."
      # Note: Not using restart here, because this sometimes fails to bind the exposed ports (which appear to be still
      # in use by the very container *itself*)...
      podman stop $CONTAINER_NAME
      podman start $CONTAINER_NAME
      return 0
  fi

  return 1
}

progress_msg "Checking if /etc/hosts file is correctly setup"
if cat /etc/hosts | grep host.docker.internal; [ $? -ne 0 ]; then
    echo "a line '127.0.0.1 host.docker.internal' must be contained in /etc/hosts file!"
    exit 1
else
    echo "OK."
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

if restart_if_possible $KEYCLOAK_CONTAINER_NAME; [ $? -ne 0 ]; then
  cd local_ca
  progress_msg "Generating certificate for $KEYCLOAK_CONTAINER_NAME"
  openssl genrsa -out "$KEYCLOAK_CONTAINER_NAME.key" 2048
  openssl req -new -key "$KEYCLOAK_CONTAINER_NAME.key" -out "$KEYCLOAK_CONTAINER_NAME.csr" \
      -subj "/C=US/ST=Local/L=Local/O=MyOrg/OU=Dev/CN=$PODMAN_SERVICE_HOSTNAME"
  sign $KEYCLOAK_CONTAINER_NAME $PODMAN_SERVICE_HOSTNAME

  cd ..

  progress_msg "Starting $KEYCLOAK_CONTAINER_NAME podman container with hostname $PODMAN_SERVICE_HOSTNAME"
  podman run \
      -d --name $KEYCLOAK_CONTAINER_NAME \
      -e KC_BOOTSTRAP_ADMIN_USERNAME=admin -e KC_BOOTSTRAP_ADMIN_PASSWORD=password \
      -e KC_HEALTH_ENABLED=true -e KC_HOSTNAME_STRICT=false \
      -e KC_HTTPS_CERTIFICATE_FILE=/opt/keycloak/ca/$KEYCLOAK_CONTAINER_NAME.crt \
      -e KC_HTTPS_CERTIFICATE_KEY_FILE=/opt/keycloak/ca/$KEYCLOAK_CONTAINER_NAME.key \
      -v "./local_ca:/opt/keycloak/ca" -v "./keycloak_realm_config:/opt/keycloak/data/import" \
      -p "0.0.0.0:8443:8443" -p "127.0.0.1:9000:9000" \
      quay.io/keycloak/keycloak:26.2 start --import-realm
fi

progress_msg "Awaiting $KEYCLOAK_CONTAINER_NAME to be healthy..."
until curl --insecure --head -fsS https://host.docker.internal:9000/health/ready --http1.1
do
    echo "--> Not yet healthy."
    sleep 5;
done
progress_msg "OK - $KEYCLOAK_CONTAINER_NAME has started."

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
  mkdir synapse_data
  # Ensure config file is owned by root in container/our user on the host machine so that we can edit it without sudo
  podman run -it --rm \
      -e UID=0 -e GID=0 \
      -v "./synapse_data:/data" \
      -e SYNAPSE_SERVER_NAME=$SYNAPSE_HOSTNAME \
      -e SYNAPSE_REPORT_STATS=no \
      docker.io/matrixdotorg/synapse:latest generate

  progress_msg "Adjusting $SYNAPSE_CONTAINER_NAME config"
  python modify_synapse_config.py

  progress_msg "Starting $SYNAPSE_CONTAINER_NAME podman container with hostname $SYNAPSE_HOSTNAME"
  podman run \
      -d --name $SYNAPSE_CONTAINER_NAME \
      -e UID=0 -e GID=0 \
      -e SSL_CERT_FILE=/opt/ca/$KIDS_CA_NAME.crt \
      -v "./local_ca:/opt/ca" -v "./synapse_data:/data" \
      -p "127.0.0.1:8048:8048" -p "127.0.0.1:$SYNAPSE_TLS_PORT:$SYNAPSE_TLS_PORT" \
      docker.io/matrixdotorg/synapse:latest

  export SHOULD_CREATE_USERS=1
fi

progress_msg "Awaiting $SYNAPSE_CONTAINER_NAME to be healthy..."
until curl --insecure --head -fsS https://host.docker.internal:8448/health
do
    echo "--> Not yet healthy."
    sleep 5;
done
progress_msg "OK - $SYNAPSE_CONTAINER_NAME has started."

if [ $SHOULD_CREATE_USERS -eq 1 ]; then
  progress_msg "Creating admin user in $SYNAPSE_CONTAINER_NAME"
  podman exec -it kids-e2e-synapse register_new_matrix_user -c /data/homeserver.yaml -u admin -p password -a

  progress_msg "Creating users in $SYNAPSE_CONTAINER_NAME"
  export ACCESS_TOKEN=$(curl --insecure -X POST https://host.docker.internal:8448/_matrix/client/v3/login \
   -d '{"identifier": { "type": "m.id.user", "user": "admin" }, "password": "password", "type": "m.login.password" }' \
   | tee /dev/stderr | jq -r .access_token)
  echo "Bearer $ACCESS_TOKEN"
  curl --insecure -X PUT https://host.docker.internal:8448/_synapse/admin/v2/users/@testuser:host.docker.internal:8448 \
    -H "Authorization: Bearer $ACCESS_TOKEN" \
    -d '{ "displayname": "Test User", "external_ids":[{ "auth_provider" : "keycloak", "external_id": "123e4567-e89b-12d3-a456-426614174000" } ] }'
  curl --insecure -X PUT https://host.docker.internal:8448/_synapse/admin/v2/users/@secondtestuser:host.docker.internal:8448 \
    -H "Authorization: Bearer $ACCESS_TOKEN" \
    -d '{ "displayname": "Second Test User", "external_ids":[{ "auth_provider" : "keycloak", "external_id": "39f5a9da-86b1-4c91-94e2-d039c928dbb4" } ] }'
  echo
fi

if restart_if_possible $SYNAPSE_ADMIN_CONTAINER_NAME; [ $? -ne 0 ]; then
  progress_msg "Starting $SYNAPSE_ADMIN_CONTAINER_NAME"
  podman run -d --name $SYNAPSE_ADMIN_CONTAINER_NAME -p 8080:80 -v "./synapse_admin_config.json:/app/config.json" ghcr.io/etkecc/synapse-admin
fi
