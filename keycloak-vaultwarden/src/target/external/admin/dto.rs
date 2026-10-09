use crate::target::types::{Email, UserId};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: UserId,
    pub email: Email,
    pub user_enabled: bool,
}
