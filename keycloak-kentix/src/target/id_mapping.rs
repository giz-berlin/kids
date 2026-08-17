pub struct UserIdMapping {
    users: std::collections::HashMap<crate::target::dto::Username, crate::target::dto::UserWithId>,
}

impl UserIdMapping {
    pub fn users(&self) -> &std::collections::HashMap<crate::target::dto::Username, crate::target::dto::UserWithId> {
        &self.users
    }
    pub fn users_mut(&mut self) -> &mut std::collections::HashMap<crate::target::dto::Username, crate::target::dto::UserWithId> {
        &mut self.users
    }
    pub async fn generate(kentix_api: &(dyn crate::target::KentixApi + Send + Sync)) -> Result<Self, kids_lib::error::KidsError> {
        tracing::trace!("Building User ID Map");
        let users = kentix_api
            .get_users()
            .await?
            .into_iter()
            .map(|user| (user.user.username.clone(), user))
            .collect();
        Ok(Self { users })
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self { users: Default::default() }
    }
}
