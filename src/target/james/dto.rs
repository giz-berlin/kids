#[derive(serde::Deserialize, Debug)]
pub struct User {
    #[serde(rename = "username")]
    pub user_email: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct Alias {
    #[serde(rename = "source")]
    pub alias_email: String,
}

#[derive(serde::Deserialize, Debug, PartialEq)]
pub struct Team {
    #[serde(rename = "name")]
    pub id: String,
    #[serde(rename = "emailAddress")]
    pub email_address: String,
}

#[derive(serde::Deserialize)]
pub struct Member {
    #[serde(default, rename = "username")]
    pub user_email: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct Group {
    pub has_group: bool,
    pub has_team: bool,
}
