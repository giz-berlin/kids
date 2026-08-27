#[derive(Debug, PartialEq, Eq, Clone, typed_builder::TypedBuilder)]
#[builder(mutators(
    pub fn with_role(&mut self, role: impl Into<String>) {
        self.roles.push(role.into());
    }
    pub fn with_group(&mut self, group: impl Into<crate::Group>) {
        self.groups.push(group.into());
    }
))]
pub struct User {
    #[builder(setter(into))]
    pub id: String,
    #[builder(default, setter(into, strip_option))]
    pub username: Option<String>,
    #[builder(default, setter(into, strip_option))]
    pub first_name: Option<String>,
    #[builder(default, setter(into, strip_option))]
    pub last_name: Option<String>,
    #[builder(default, setter(into, strip_option))]
    pub email: Option<String>,
    pub enabled: bool,
    #[builder(default, setter(into))]
    pub attributes: std::collections::HashMap<String, Vec<String>>,
    #[builder(via_mutators)]
    pub groups: Vec<crate::Group>,
    #[builder(via_mutators)]
    pub roles: Vec<String>,
}

#[async_trait::async_trait]
impl kids_lib::interface::source::User for User {
    fn id(&self) -> &kids_lib::types::SharedResourceIdentifier {
        &self.id
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }
    fn first_name(&self) -> Option<&str> {
        self.first_name.as_deref()
    }
    fn last_name(&self) -> Option<&str> {
        self.last_name.as_deref()
    }
    fn email(&self) -> Option<&str> {
        self.email.as_ref().map(|s| s.as_ref())
    }
    fn attributes(&self) -> &std::collections::HashMap<String, Vec<String>> {
        &self.attributes
    }

    async fn groups(
        &self,
        _include_transitive_groups: bool,
    ) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync>>, kids_lib::error::KidsError> {
        Ok(self.groups.clone().into_iter().map(Into::into).collect())
    }

    async fn roles(&self) -> Result<Vec<String>, kids_lib::error::KidsError> {
        Ok(self.roles.to_vec())
    }
}
