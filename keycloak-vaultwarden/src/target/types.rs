//! Values shared between the [interactor](super::interactor) and the [external](super::external) clients.

/// Creates a string ID newtype.
macro_rules! Id {
    ( $id_name:ident ) => {
        #[doc = concat!("A unique identifier ", stringify!($id_name), ".")]
        #[derive(Debug, PartialEq, Eq, Clone, Hash, PartialOrd, Ord, serde::Deserialize, serde::Serialize)]
        pub struct $id_name(String);

        impl $id_name {
            pub const fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        impl std::fmt::Display for $id_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl PartialEq<&$id_name> for $id_name {
            fn eq(&self, other: &&$id_name) -> bool {
                self == *other
            }
        }
    };
}

Id!(GroupId);
Id!(CollectionId);
Id!(UserId);
Id!(MembershipId);

/// Status of a user's membership in an organization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde_repr::Deserialize_repr)]
#[repr(i8)]
pub enum MembershipStatus {
    /// User's membership has been revoked, i.e. no longer has access.
    Revoked = -1,
    /// User has been invited.
    /// If SMTP is configured the user has received an invitation email.
    /// If SMTP is not configured and their account exists, they would immediately be [Accepted](MembershipStatus::Accepted),
    /// so the user doesn't exist yet.
    Invited = 0,
    /// The invitation was accepted, but the member still has to be confirmed before it gains access.
    Accepted = 1,
    /// Confirmed member of organization with access.
    Confirmed = 2,
}

/// Type of a user's membership in an organization.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum MembershipType {
    Owner = 0,
    Admin = 1,
    User = 2,
    Manager = 3,
}

/// Permissions a user/group has for a collection.
/// Vaultwarden handles these in a super confusing way. There are code comments that say
/// "If both read_only and hide_passwords are false, then manage should be true" ([Link](https://github.com/dani-garcia/vaultwarden/blob/1f802f8e6ae9b788e1e9cc5dffb7e83780417332/src/db/models/group.rs#L87)),
/// but the UI dropdown supports the following options:
/// | Dropdown option              | readOnly | hidePasswords | manage |
/// |------------------------------|----------|---------------|--------|
/// | View items, hidden passwords | true     | true          | false  |
/// | View items                   | true     | false         | false  |
/// | Edit items, hidden passwords | false    | true          | false  |
/// | Edit items                   | false    | false         | false  |
/// | Manage collection            | false    | false         | true   |
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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

/// An entry of a group access list, for a group.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GroupAccess {
    pub id: GroupId,
    #[serde(flatten)]
    pub flags: AccessFlags,
}

/// An entry of a collection access list, for a collection.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CollectionAccess {
    pub id: CollectionId,
    #[serde(flatten)]
    pub flags: AccessFlags,
}

/// An entry of a collection's access list, for a member with direct access, not through a group.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct MemberAccess {
    pub id: MembershipId,
    #[serde(flatten)]
    pub flags: AccessFlags,
}

/// An email address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub struct Email(String);

impl From<&str> for Email {
    fn from(email: &str) -> Self {
        Self(email.to_owned())
    }
}

impl std::fmt::Display for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
