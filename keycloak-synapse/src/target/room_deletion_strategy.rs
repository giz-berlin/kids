#[derive(serde::Deserialize, Debug, Clone, Copy)]
pub enum RoomDeletionStrategy {
    /// Do not modify the Matrix room. No users will be removed from it
    /// and no attributes (name, alias, power levels) will be updated.
    Ignore,
    /// Kick all users except the sync user from the room. The room will continue to exist and
    /// users may be added again later. Depending on the Matrix homeserver configuration, users
    /// might access messages again if they could read/decrypt them before they were kicked.
    KickAll,
    /// Like [KickAll](RoomDeletionStrategy::KickAll), but the sync user also leaves the room.
    /// With no members left, it is **impossible to re-add members**, the room is "bricked".
    /// Synapse will shut down the room after some time and might delete messages from the database.
    Evacuate,
    /// The most effective and dangerous option. The room is completely deleted and all traces of
    /// it are removed from the database immediately.
    Delete,
}
