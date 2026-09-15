use crate::target::types::{Access, MembershipStatus};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub user_id: String,
    pub email: String,
    pub external_id: Option<String>,
    pub status: MembershipStatus,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    pub external_id: Option<String>,
    pub access_all: bool,
    pub collections: Vec<Access>,
}

#[derive(serde::Deserialize)]
pub struct ListResponse<T> {
    pub data: Vec<T>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRequest<'a> {
    pub name: &'a str,
    pub access_all: bool,
    pub external_id: Option<&'a str>,
    pub collections: &'a [Access],
    pub users: &'a [String],
}

#[derive(serde::Deserialize)]
pub struct CreatedGroup {
    pub id: String,
}

#[derive(serde::Deserialize)]
pub struct Collection {
    pub id: String,
}

#[derive(serde::Deserialize)]
pub struct Profile {
    pub email: String,
}
