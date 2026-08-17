#[derive(Debug, Default)]
pub struct KentixApiMocker {
    api_mock: crate::target::external::MockKentixApi,
    levelprofiles: Vec<crate::target::dto::Levelprofile>,
    kentix_users: Vec<crate::target::dto::UserWithId>,
}

pub const EXPLICITLY_FORBIDDEN_METHOD: &str = "This call is explicitly forbidden by the test setup";

impl KentixApiMocker {
    pub fn with_levelprofiles(mut self, levelprofiles: impl Into<Vec<crate::target::dto::Levelprofile>>) -> Self {
        self.levelprofiles = levelprofiles.into();
        self
    }

    pub fn with_users(mut self, kentix_users: impl Into<Vec<crate::target::dto::UserWithId>>) -> Self {
        self.kentix_users = kentix_users.into();
        self
    }

    pub fn can_get_all_levelprofiles(mut self) -> Self {
        let levelprofiles = self.levelprofiles.clone();
        self.api_mock.expect_get_levelprofiles().returning(move || Ok(levelprofiles.clone()));
        self
    }

    pub fn can_get_all_users(mut self) -> Self {
        let users = self.kentix_users.clone();
        self.api_mock.expect_get_users().returning(move || Ok(users.clone()));
        self
    }

    pub fn errors_get_all_users(mut self) -> Self {
        self.api_mock
            .expect_get_users()
            .returning(|| Err(kids_lib::error::KidsError::InternalError(EXPLICITLY_FORBIDDEN_METHOD.to_owned())));
        self
    }

    pub fn require_create_user(mut self, user: crate::target::dto::User, user_id: impl Into<crate::target::dto::UserId> + Send + 'static) -> Self {
        self.api_mock
            .expect_create_user()
            .with(mockall::predicate::eq(user))
            .times(1)
            .return_once(|user| Ok(crate::target::dto::UserWithId { id: user_id.into(), user }));
        self
    }

    pub fn require_update_user(mut self, user: crate::target::dto::UserWithId) -> Self {
        self.api_mock.expect_update_user().with(mockall::predicate::eq(user)).times(1).return_once(Ok);
        self
    }

    pub fn errors_update_user(mut self, user: crate::target::dto::UserWithId) -> Self {
        self.api_mock
            .expect_update_user()
            .with(mockall::predicate::eq(user))
            .returning(|_| Err(kids_lib::error::KidsError::InternalError(EXPLICITLY_FORBIDDEN_METHOD.to_owned())));
        self
    }

    pub fn require_delete_user(mut self, user: crate::target::dto::UserWithId) -> Self {
        self.api_mock
            .expect_delete_user()
            .with(mockall::predicate::eq(user))
            .times(1)
            .return_once(|_| Ok(()));
        self
    }
}

impl From<KentixApiMocker> for Box<dyn crate::target::external::KentixApi + Send + Sync> {
    fn from(val: KentixApiMocker) -> Self {
        Box::new(val.api_mock)
    }
}
