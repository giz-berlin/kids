#!/bin/bash
# Verifies the state in vaultwarden after kids has synced the keycloak realm.

set -o errexit
set -o pipefail
set -o allexport
set -o nounset

cd "$(dirname "$(realpath "$0")")"
source .env

URL="https://$PODMAN_SERVICE_HOSTNAME:$VAULTWARDEN_TLS_PORT"
# The certificate of vaultwarden is self-signed
CURL="curl -fsSk"

# Checks whether a variable contains an expected value.
# $1 - name of the assertion we are making / variable we are checking
# $2 - actual value of the variable
# $3 - expected value of the variable
compare() {
  if [ "$2" = "$3" ]; then
    echo "$1 is $2, as expected."
  else
    echo "$1 is $2, expected $3."
    exit 1
  fi
}

TOKEN=$($CURL -d grant_type=client_credentials -d scope=api -d client_id="$VAULTWARDEN_USER_CLIENT_ID" \
  -d client_secret="$VAULTWARDEN_USER_CLIENT_SECRET" -d deviceType=9 -d deviceIdentifier="$(cat /proc/sys/kernel/random/uuid)" \
  -d deviceName=verify "$URL/identity/connect/token" | jq -r .access_token)
api() { $CURL -H "Authorization: Bearer $TOKEN" "$URL/api/organizations/$VAULTWARDEN_ORG_ID/$1"; }

GROUPS_JSON=$(api groups)
compare "Groups" "$(jq -r '[.data[].name] | sort | join(",")' <<<"$GROUPS_JSON")" \
  "TestGroup,TestGroup/SubGroup,TestGroup/The other SubGroup"

MEMBERS=$(api "users?includeGroups=true")

member_status() { jq -r --arg email "$1" '.data[] | select(.email == $email).status' <<<"$MEMBERS"; }
member_groups() {
  jq -r --arg email "$1" --argjson groups "$GROUPS_JSON" \
    '($groups.data | map({(.id): .name}) | add) as $names
     | .data[] | select(.email == $email) | [.groups[] | $names[.]] | sort | join(",")' <<<"$MEMBERS"
}


compare "Status of existing account user@test.giz.berlin" "$(member_status user@test.giz.berlin)" 2
compare "Groups of existing account user@test.giz.berlin" "$(member_groups user@test.giz.berlin)" "TestGroup"
compare "Status of new account tester@test.giz.berlin" "$(member_status tester@test.giz.berlin)" 0
compare "Groups of new account tester@test.giz.berlin" "$(member_groups tester@test.giz.berlin)" "TestGroup,TestGroup/SubGroup"
compare "Status of the syncer account" "$(member_status "$VAULTWARDEN_SYNCER_EMAIL")" 2
compare "Groups of the syncer account" "$(member_groups "$VAULTWARDEN_SYNCER_EMAIL")" ""
