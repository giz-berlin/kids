#[allow(unused_imports)]
pub use internal::{
    Levelprofile, LevelprofileId, LevelprofileName, PaginatedResponse, User, UserActive, UserEmail, UserEmergencyAccess, UserFullName, UserId, UserRfidData,
    UserRfidUid, UserWithId, Username,
};

/// This module contains all types related to the [dtos](crate::target::dto).
/// You should normally don't have to use this directly, use the re-exports instead.
mod internal {
    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct LevelprofileId(pub i32);

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Hash)]
    pub struct LevelprofileName(pub String);

    /// Also known as access profile.
    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone)]
    #[cfg_attr(test, derive(typed_builder::TypedBuilder))]
    #[serde(deny_unknown_fields)]
    pub struct Levelprofile {
        #[cfg_attr(test, builder(setter(into)))]
        pub id: LevelprofileId,
        #[cfg_attr(test, builder(setter(into)))]
        pub name: LevelprofileName,
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct UserId(pub i32);
    impl std::fmt::Display for UserId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            std::fmt::Display::fmt(&self.0, f)
        }
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Hash)]
    pub struct Username(pub String);
    impl std::fmt::Display for Username {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            std::fmt::Display::fmt(&self.0, f)
        }
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Hash)]
    pub struct UserFullName(pub String);

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct UserActive(pub bool);

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Hash)]
    pub struct UserEmail(pub String);

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct UserEmergencyAccess(pub bool);

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
    pub struct UserRfidUid(#[serde(serialize_with = "as_hex", deserialize_with = "from_hex")] u128);

    impl TryFrom<&str> for UserRfidUid {
        type Error = std::num::ParseIntError;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            let uid = u128::from_str_radix(value, 16)?;
            Ok(Self(uid))
        }
    }

    impl TryFrom<&String> for UserRfidUid {
        type Error = std::num::ParseIntError;

        fn try_from(value: &String) -> Result<Self, Self::Error> {
            value.as_str().try_into()
        }
    }

    // This has to ne here instead of in `dto_test_helpers.rs` as
    // otherwise we cannot access the non-`pub` field.
    #[cfg(test)]
    impl From<u128> for UserRfidUid {
        fn from(value: u128) -> Self {
            Self(value)
        }
    }

    fn as_hex<S>(int: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{:x}", int))
    }

    fn from_hex<'de, D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        use serde::de::Deserialize;
        use serde::de::Error;
        String::deserialize(deserializer).and_then(|string| u128::from_str_radix(string.as_ref(), 16).map_err(|err| Error::custom(err.to_string())))
        // .map(|bytes| PublicKey::from_slice(&bytes))
        // .and_then(|opt| opt.ok_or_else(|| Error::custom("failed to deserialize public key")))
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone, Hash)]
    pub struct UserRfidData(pub String);

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone)]
    #[cfg_attr(test, derive(typed_builder::TypedBuilder))]
    #[cfg_attr(test, builder(mutators(
        pub fn with_levelprofile(&mut self, levelprofile_id: impl Into<LevelprofileId>) {
            self.levelprofiles.push(levelprofile_id.into());
        }
    )))]
    pub struct User {
        /// A distinct username across all users.
        #[cfg_attr(test, builder(setter(into)))]
        pub username: Username,
        /// A variable name of the user, not necessarily unique.
        #[cfg_attr(test, builder(setter(into)))]
        pub fullname: UserFullName,
        #[cfg_attr(test, builder(setter(into)))]
        #[serde(alias = "active")]
        pub is_active: UserActive,
        #[cfg_attr(test, builder(default, setter(into, strip_option)))]
        pub email: Option<UserEmail>,
        #[cfg_attr(test, builder(setter(into)))]
        pub emergency_access: UserEmergencyAccess,
        #[cfg_attr(test, builder(via_mutators))]
        pub levelprofiles: Vec<LevelprofileId>,
        #[cfg_attr(test, builder(setter(into, strip_option)))]
        pub rfid_uid: Option<UserRfidUid>,
        #[cfg_attr(test, builder(setter(into, strip_option)))]
        pub rfid_data: Option<UserRfidData>,
        // This struct has more fields that we ignore.
    }

    #[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq, Clone)]
    #[cfg_attr(test, derive(typed_builder::TypedBuilder))]
    pub struct UserWithId {
        /// A Kentix-internal ID of the user.
        #[cfg_attr(test, builder(setter(into)))]
        pub id: UserId,
        /// The remaining properties.
        #[serde(flatten)]
        pub user: User,
    }

    #[derive(Debug, serde::Deserialize)]
    pub struct PaginatedMetadata {
        pub per_page: String,
        pub total: i32,
        // This struct has more fields that we ignore.
    }

    #[derive(Debug, serde::Deserialize)]
    pub struct PaginatedResponse<T> {
        pub data: Vec<T>,
        pub meta: PaginatedMetadata,
        // This struct has more fields that we ignore.
    }
}
