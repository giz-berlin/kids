#[derive(Debug, PartialEq, Eq, serde::Deserialize, Clone, Copy)]
pub struct UserId(u32);

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl From<UserId> for serde_json::Value {
    fn from(value: UserId) -> Self {
        value.0.into()
    }
}

#[derive(Debug, PartialEq, Eq, serde::Deserialize, Clone, Copy)]
pub struct RoleId(u32);

impl RoleId {
    pub const fn id(self) -> u32 {
        self.0
    }
}

impl From<RoleId> for serde_json::Value {
    fn from(value: RoleId) -> Self {
        value.0.into()
    }
}

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
pub struct RoleName(String);

#[derive(Debug, PartialEq, Eq, serde::Deserialize, Clone, Copy)]
pub struct UserActive(bool);

impl From<bool> for UserActive {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

impl From<UserActive> for serde_json::Value {
    fn from(value: UserActive) -> Self {
        value.0.into()
    }
}

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
pub struct User {
    id: UserId,
    role_ids: Vec<RoleId>,
    active: UserActive,
    #[serde(flatten)]
    additional_properties: std::collections::HashMap<String, serde_json::Value>,
}
