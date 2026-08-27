#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Group {
    pub id: String,
    pub attributes: std::collections::HashMap<String, Vec<String>>,
}

impl Group {
    pub fn new(id: impl Into<String>, attributes: Option<std::collections::HashMap<String, Vec<String>>>) -> Self {
        Self {
            id: id.into(),
            attributes: attributes.unwrap_or_default(),
        }
    }
}

impl From<Group> for std::sync::Arc<dyn kids_lib::interface::source::Group + Send + Sync> {
    fn from(value: Group) -> Self {
        std::sync::Arc::new(value)
    }
}

#[async_trait::async_trait(?Send)]
impl kids_lib::interface::source::Group for Group {
    fn id(&self) -> &kids_lib::types::SharedResourceIdentifier {
        &self.id
    }

    fn name(&self) -> &str {
        &self.id
    }

    fn path(&self) -> &str {
        &self.id
    }

    fn attributes(&self) -> &std::collections::HashMap<String, Vec<String>> {
        &self.attributes
    }

    fn root_group(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn kids_lib::interface::source::Group> {
        self
    }

    fn parent_group(&self) -> Option<std::sync::Arc<dyn kids_lib::interface::source::Group>> {
        None
    }

    async fn sub_groups(self: std::sync::Arc<Self>) -> Result<Vec<std::sync::Arc<dyn kids_lib::interface::source::Group>>, kids_lib::error::KidsError> {
        Ok(vec![])
    }
}
