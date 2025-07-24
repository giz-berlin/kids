#!/bin/bash

source .env

podman stop $KEYCLOAK_CONTAINER_NAME && podman rm $KEYCLOAK_CONTAINER_NAME
podman stop $SYNAPSE_CONTAINER_NAME && podman rm $SYNAPSE_CONTAINER_NAME
podman stop $SYNAPSE_ADMIN_CONTAINER_NAME && podman rm $SYNAPSE_ADMIN_CONTAINER_NAME
rm -rf local_ca
rm -rf synapse_data
