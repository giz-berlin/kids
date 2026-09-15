use crate::target::types::Access;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub external_id: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionTemplate<'a> {
    pub organization_id: &'a str,
    pub name: &'a str,
    pub external_id: &'a str,
    pub groups: &'a [Access],
    pub users: &'a [Access],
}

#[derive(serde::Deserialize)]
pub struct CreatedCollection {
    pub id: String,
}
