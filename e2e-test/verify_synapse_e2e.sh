#!/usr/bin/env bash

set -e
set -a

progress_msg() {
  # Color in blue
  printf "\033[0;34m# %s \033[0m\n" "$1"
}

compare() {
  PROPERTY="$1"
  KEY="$2"
  ACTUAL="$3"
  EXPECTED="$4"

  if [ "$ACTUAL" = "$EXPECTED" ]; then
    echo "$PROPERTY of $KEY is $ACTUAL, as expected."
  else
    echo "$PROPERTY of $KEY is $ACTUAL, expected $EXPECTED."
    exit 1
  fi
}

check_user_display_name() {
  USER_NAME="$1"
  EXPECTED_DISPLAY_NAME="$2"
  SYNADM_CONFIG_FILE="$3"
  ACTUAL_DISPLAY_NAME=$(synadm --config-file "$SYNADM_CONFIG_FILE" user list | jq -r ".users.[] | select(.name == \"$USER_NAME\").displayname")
  compare "Display name" "$USER_NAME" "$ACTUAL_DISPLAY_NAME" "$EXPECTED_DISPLAY_NAME"
}

check_room() {
  ROOM_ID="$1"
  EXPECTED_NAME="$2"
  EXPECTED_CANONICAL_ALIAS="$3"
  EXPECTED_CREATOR="$4"
  EXPECTED_NUMBER_OF_MEMBERS="$5"
  SYNADM_CONFIG_FILE="$6"

  RESPONSE=$(synadm --config-file "$SYNADM_CONFIG_FILE" room details "$ROOM_ID")

  ACTUAL_NAME=$(echo $RESPONSE | jq -r '.name')
  compare "Name" "$ROOM_ID" "$ACTUAL_NAME" "$EXPECTED_NAME"

  ACTUAL_CANONICAL_ALIAS=$(echo $RESPONSE | jq -r '.canonical_alias')
  compare "Canonical alias" "$ROOM_ID" "$ACTUAL_CANONICAL_ALIAS" "$EXPECTED_CANONICAL_ALIAS"

  ACTUAL_CREATOR=$(echo $RESPONSE | jq -r '.creator')
  compare "Creator" "$ROOM_ID" "$ACTUAL_CREATOR" "$EXPECTED_CREATOR"

  # We always expect the same number of local and remote members as we should not have remote members.
  ACTUAL_NUMBER_OF_MEMBERS=$(echo $RESPONSE | jq -r '.joined_local_members')
  compare "Number of local members" "$ROOM_ID" "$ACTUAL_NUMBER_OF_MEMBERS" "$EXPECTED_NUMBER_OF_MEMBERS"

  ACTUAL_NUMBER_OF_MEMBERS=$(echo $RESPONSE | jq -r '.joined_members')
  compare "Number of members" "$ROOM_ID" "$ACTUAL_NUMBER_OF_MEMBERS" "$EXPECTED_NUMBER_OF_MEMBERS"
}

check_room_membership() {
  ROOM_ID="$1"
  USER_ID="$2"
  SYNADM_CONFIG_FILE="$3"

  MEMBERS_TMP_FILE=$(mktemp)
  synadm --config-file "$SYNADM_CONFIG_FILE" room members "$ROOM_ID" | jq -r '.members.[]' > "$MEMBERS_TMP_FILE"
  if grep "^$USER_ID$" "$MEMBERS_TMP_FILE"; [ $? -ne 0 ]; then
    echo "User $USER_ID is not member of the room $ROOM_ID despite being expected."
    exit 1
  else
    echo "User $USER_ID is member of the room $ROOM_ID, as expected."
  fi
}

progress_msg "Set up synadm config."
# Ignore HTTPS errors as we access a local server without a valid certificate.
export PYTHONWARNINGS="ignore:Unverified HTTPS request"
SYNADM_CONFIG_FILE=$(mktemp)
echo "admin_path: /_synapse/admin" >> "$SYNADM_CONFIG_FILE"
echo "base_url: https://$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT" >> "$SYNADM_CONFIG_FILE"
echo "format: json" >> "$SYNADM_CONFIG_FILE"
echo "homeserver: auto-retrieval" >> "$SYNADM_CONFIG_FILE"
echo "matrix_path: /_matrix" >> "$SYNADM_CONFIG_FILE"
echo "protocol: http" >> "$SYNADM_CONFIG_FILE"
echo "server_discovery: well-known" >> "$SYNADM_CONFIG_FILE"
echo "ssl_verify: false" >> "$SYNADM_CONFIG_FILE"
echo "timeout: 30" >> "$SYNADM_CONFIG_FILE"
echo "token: invalid" >> "$SYNADM_CONFIG_FILE"
echo "user: '@admin:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT'" >> "$SYNADM_CONFIG_FILE"

progress_msg "Login to Synapse"
SYNAPSE_TOKEN=$(synadm --config-file "$SYNADM_CONFIG_FILE" matrix login "@admin:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT" --password "password" | jq -r '.access_token')
sed -i -e "s/token: invalid/token: $SYNAPSE_TOKEN/g" "$SYNADM_CONFIG_FILE"

progress_msg "Verify users"
synadm --config-file "$SYNADM_CONFIG_FILE" user list | jq
TOTAL_NUMBER_OF_USERS=$(synadm --config-file "$SYNADM_CONFIG_FILE" user list | jq '.total')
EXPECTED_NUMBER_OF_USERS=3
if [ $TOTAL_NUMBER_OF_USERS -ne $EXPECTED_NUMBER_OF_USERS ]; then
  echo "Found $TOTAL_NUMBER_OF_USERS users, expected $EXPECTED_NUMBER_OF_USERS."
  exit 1
fi

ADMIN_USER_NAME="@admin:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT"
TESTUSER_USER_NAME="@testuser:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT"
SECOND_TESTUSER_USER_NAME="@secondtestuser:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT"
check_user_display_name "$ADMIN_USER_NAME" "admin" "$SYNADM_CONFIG_FILE"
check_user_display_name "$SECOND_TESTUSER_USER_NAME" "Second Tester" "$SYNADM_CONFIG_FILE"
check_user_display_name "$TESTUSER_USER_NAME" "First Tester" "$SYNADM_CONFIG_FILE"

progress_msg "Verify rooms"
synadm --config-file "$SYNADM_CONFIG_FILE" room list | jq
TOTAL_NUMBER_OF_ROOMS=$(synadm --config-file "$SYNADM_CONFIG_FILE" room list | jq '.total_rooms')
EXPECTED_NUMBER_OF_ROOMS=2
if [ $TOTAL_NUMBER_OF_ROOMS -ne $EXPECTED_NUMBER_OF_ROOMS ]; then
  echo "Found $TOTAL_NUMBER_OF_ROOMS rooms, expected $EXPECTED_NUMBER_OF_ROOMS."
  exit 1
fi

# We cannot match the room ID to the desired room state without using the canonical alias.
PARENT_GROUP_ROOM_ID=$(synadm --config-file "$SYNADM_CONFIG_FILE" room list | jq -r ".rooms.[] | select(.canonical_alias == \"#testgroup:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT\").room_id")
CHILD_GROUP_ROOM_ID=$(synadm --config-file "$SYNADM_CONFIG_FILE" room list | jq -r ".rooms.[] | select(.canonical_alias == \"#testgroup-subgroup:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT\").room_id")
check_room "$PARENT_GROUP_ROOM_ID" "Test Group" "#testgroup:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT" "$ADMIN_USER_NAME" 3 "$SYNADM_CONFIG_FILE"
check_room "$CHILD_GROUP_ROOM_ID" "SubGroup" "#testgroup-subgroup:$PODMAN_SERVICE_HOSTNAME:$SYNAPSE_TLS_PORT" "$ADMIN_USER_NAME" 2 "$SYNADM_CONFIG_FILE"

progress_msg "Verify room memberships"
check_room_membership "$PARENT_GROUP_ROOM_ID" "$ADMIN_USER_NAME" "$SYNADM_CONFIG_FILE"
check_room_membership "$CHILD_GROUP_ROOM_ID" "$ADMIN_USER_NAME" "$SYNADM_CONFIG_FILE"

check_room_membership "$PARENT_GROUP_ROOM_ID" "$SECOND_TESTUSER_USER_NAME" "$SYNADM_CONFIG_FILE"
check_room_membership "$PARENT_GROUP_ROOM_ID" "$TESTUSER_USER_NAME" "$SYNADM_CONFIG_FILE"

check_room_membership "$CHILD_GROUP_ROOM_ID" "$TESTUSER_USER_NAME" "$SYNADM_CONFIG_FILE"

progress_msg "All tests passed!"
