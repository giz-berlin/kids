//! Values shared between the [interactor](super::interactor) and the [external](super::external) clients.

#[derive(PartialEq, Eq, serde_repr::Deserialize_repr)]
#[repr(i8)]
pub enum MembershipStatus {
    Revoked = -1,
    Invited = 0,
    /// The invitation was accepted, but the member still has to be confirmed before it gains access.
    Accepted = 1,
    Confirmed = 2,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessFlags {
    pub read_only: bool,
    pub hide_passwords: bool,
    pub manage: bool,
}

impl AccessFlags {
    pub const EDIT: Self = Self {
        read_only: false,
        hide_passwords: false,
        manage: false,
    };
}

/// An entry of an access list. On a group, `id` is a collection the group can access. On a
/// collection, `id` is a group or member that can access it.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Access {
    pub id: String,
    #[serde(flatten)]
    pub flags: AccessFlags,
}
