#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportMember<'a> {
    pub email: &'a str,
    pub external_id: &'a str,
    pub deleted: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest<'a> {
    /// Always empty, groups are managed through the vault API.
    pub groups: &'a [serde_json::Value],
    pub members: &'a [ImportMember<'a>],
    pub overwrite_existing: bool,
}
