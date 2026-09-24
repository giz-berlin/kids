#!/bin/bash

source .env

podman stop $KEYCLOAK_CONTAINER_NAME && podman rm $KEYCLOAK_CONTAINER_NAME
podman stop $SYNAPSE_CONTAINER_NAME && podman rm $SYNAPSE_CONTAINER_NAME
podman stop $MAS_CONTAINER_NAME && podman rm $MAS_CONTAINER_NAME
podman stop $MAS_POSTGRES_CONTAINER_NAME && podman rm $MAS_POSTGRES_CONTAINER_NAME
podman stop $SYNAPSE_ADMIN_CONTAINER_NAME && podman rm $SYNAPSE_ADMIN_CONTAINER_NAME
podman stop $REVERSE_PROXY_CONTAINER_NAME && podman rm $REVERSE_PROXY_CONTAINER_NAME
rm -rf local_ca
rm -rf synapse_data
rm -f config/keycloak_realm_giz.json
rm -f config/synapse_admin_config.json
rm -f config/synapse_e2e_config.toml
rm -f config/synapse_admin.Caddyfile
rm -f .env.token
rm -rf caddy_data/
rm -rf caddy_config/
