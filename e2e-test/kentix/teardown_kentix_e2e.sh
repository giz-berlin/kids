#!/bin/bash

source .env

podman stop $KEYCLOAK_CONTAINER_NAME && podman rm $KEYCLOAK_CONTAINER_NAME
rm -rf local_ca
rm -f config/keycloak_realm_giz.json
rm -f config/kentix_e2e_config.toml
