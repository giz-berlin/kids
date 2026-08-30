# Synapse Target

Synchronizes source groups to Matrix rooms on a Synapse homeserver.

## Working principles

For each source group with the `source_room_name_attr` (see [configuration](src/target/connector.rs)), a room with the same members
will be created and managed.
Users added to the group will be [force-joined](https://matrix-org.github.io/synapse/latest/admin_api/room_membership.html)
to the room and users removed will be kicked. Matrix allows users to leave any room at any time, but they will be re-joined
on the next sync.
Users with the `required_role_name` (see [configuration](src/target/connector.rs)) will be created if they do not exist already.
They will be joined to their rooms immediately.
**Note** that users need to login once before being able to read any messages in any room using encryption.

Matrix requires that any action with regard to a room must be initiated by a user with sufficient access rights
([power levels](https://spec.matrix.org/v1.5/client-server-api/#permissions)).
Therefore, an ideally dedicated Keycloak user (sync user) will be in every room the syncer creates to perform most of the
synchronization actions.
The sync user will store the source group ID in the metadata of a room to match them for future syncs.
A room is considered *managed* by the syncer if the sync user is a member and can find a source group id in the room's metadata.
To resolve Matrix users to Keycloak users, force-join them to rooms and optionally delete rooms, the sync user must have
administrator privileges in Synapse.

The name of the room is kept in sync with the group attribute value and a human-readable alias based on the group path will be assigned.
The power levels are set such that any user can send messages, change the room topic and avatar (room picture) but not kick or invite other users.
Encryption will be enabled.

The syncer ignores non-managed rooms, even if the sync user is a member of them. This means users can create their own rooms,
send private messages and so on without the syncer interfering.

## Configuration

Please refer to the [configuration example file](../default_configs/synapse-target.config.example.toml).
