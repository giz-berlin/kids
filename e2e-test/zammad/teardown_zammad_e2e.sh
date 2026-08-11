#!/bin/bash

source .env

podman stop $KEYCLOAK_CONTAINER_NAME && podman rm $KEYCLOAK_CONTAINER_NAME
pushd zammad-docker-compose
podman compose down -v
popd
rm -rf local_ca
rm -rf zammad_data
rm -rf zammad-docker-compose
rm config/keycloak_realm_giz.json
rm config/zammad_e2e_config.toml
