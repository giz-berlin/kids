#[derive(serde::Deserialize, Debug)]
pub struct User {
    pub username: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct Alias {
    pub source: String,
}

#[derive(serde::Deserialize, Debug, PartialEq)]
pub struct Team {
    pub name: String,
    #[serde(rename = "emailAddress")]
    pub email_address: String,
}

#[derive(serde::Deserialize)]
pub struct Member {
    #[serde(default)]
    pub username: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct Group {
    pub has_group: bool,
    pub has_team: bool,
}
