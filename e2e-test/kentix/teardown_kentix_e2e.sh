#!/bin/bash

source .env

podman stop $KEYCLOAK_CONTAINER_NAME && podman rm $KEYCLOAK_CONTAINER_NAME
rm -rf local_ca
rm config/keycloak_realm_giz.json
rm config/kentix_e2e_config.toml
