#!/bin/bash
# Removes everything setup.sh and `podman compose up` created.

cd "$(dirname "$(realpath "$0")")"

podman compose down --volumes

rm -rf certs
rm -rf vw_data
rm -f config/keycloak_realm_giz.json
rm -f config/kids_config.toml
