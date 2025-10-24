/// This struct contains no fields. When deserializing to this type, we intentionally throw
/// away all information contained in the original string.
#[derive(serde::Deserialize)]
pub struct IgnoredResponse {}

#[derive(serde::Deserialize, Debug)]
pub struct User {
    pub username: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct ListUsersResponse {
    pub users: Vec<User>,
}

#[derive(serde::Deserialize, Debug)]
pub struct ListGroupsResponse {
    pub groups: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct ListGroupMembersResponse {
    pub members: Vec<String>,
}

#[derive(serde::Deserialize, Debug)]
pub struct Alias {
    pub source: String,
}

#[derive(serde::Deserialize, Debug)]
#[derive(PartialEq)]
pub struct Team {
    pub name: String,
    pub emailAddress: String,
}

#[derive(serde::Deserialize)]
pub struct Member {
    #[serde(default)]
    pub username: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct Group {
    pub email: String,
    pub has_group: bool,
    pub has_team: bool,
}
