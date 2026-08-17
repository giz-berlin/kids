//! This module provides implementation for the [dtos](crate::target::dto) that
//! are only simplifications for tests and should not be used in production code.

impl From<i32> for crate::target::dto::LevelprofileId {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl<S: Into<String>> From<S> for crate::target::dto::LevelprofileName {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

impl From<i32> for crate::target::dto::UserId {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl<S: Into<String>> From<S> for crate::target::dto::Username {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

impl<S: Into<String>> From<S> for crate::target::dto::UserFullName {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

impl From<bool> for crate::target::dto::UserActive {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

impl<S: Into<String>> From<S> for crate::target::dto::UserEmail {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

impl From<bool> for crate::target::dto::UserEmergencyAccess {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

impl<S: Into<String>> From<S> for crate::target::dto::UserRfidData {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}
