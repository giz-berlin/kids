use crate::target::types::{CollectionAccess, CollectionId, Email, GroupId, MembershipId, MembershipStatus, MembershipType, UserId};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMembershipRequest<'a> {
    #[serde(rename = "type")]
    pub membership_type: &'a MembershipType,
    pub collections: &'a [CollectionAccess],
    pub groups: &'a [GroupId],
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Membership {
    /// Primary ID of the [Membership]
    pub id: MembershipId,
    /// ID of the user account
    pub user_id: UserId,
    pub email: Email,
    /// Mapped Keycloak ID
    pub external_id: Option<String>,
    pub status: MembershipStatus,
    #[serde(rename = "type")]
    pub membership_type: MembershipType,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub external_id: Option<String>,
    pub access_all: bool,
    pub collections: Vec<CollectionAccess>,
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
    pub collections: &'a [CollectionAccess],
    pub users: &'a [MembershipId],
}

#[derive(serde::Deserialize)]
pub struct CreatedGroup {
    pub id: GroupId,
}

#[derive(serde::Deserialize)]
pub struct Collection {
    pub id: CollectionId,
}

#[derive(serde::Deserialize)]
pub struct Profile {
    pub email: Email,
}
