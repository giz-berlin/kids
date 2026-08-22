use crate::error::KidsError;
use crate::target::synapse::external::MockSynapseApi;
use crate::target::synapse::{dto, external};
use crate::test_util::constants;
use crate::{error, test_util};
use mockall::predicate::eq;

// The builder macro appears to confuse clippy in some way.
// For example, it thinks the build_into() methods and the entity_number fields are unused, but they aren't.
#[allow(dead_code)]
#[derive(derive_builder::Builder, Default, Debug, Clone)]
#[builder(build_fn(name = "build_fallible"), setter(into), default)]
pub struct MockSynapseRoom {
    #[builder(field(ty = "test_util::RandomId"))]
    entity_number: test_util::RandomId,

    #[builder(default = "self.default_room_id()")]
    pub matrix_room_id: String,
    #[builder(default = "uuid::Uuid::new_v4().into()")]
    pub source_room_id: String,
    #[builder(default = "self.default_alias()")]
    pub alias: String,
    #[builder(default = "self.default_name()")]
    pub name: String,
    #[builder(setter(each(name = "user")))]
    pub users: Vec<String>,
}

impl MockSynapseRoomBuilder {
    fn default_room_id(&self) -> String {
        format!("!roomId{}:{}", self.entity_number, constants::DEFAULT_MATRIX_HOMESERVER)
    }

    fn default_alias(&self) -> String {
        format!("#Room{}:{}", self.entity_number, constants::DEFAULT_MATRIX_HOMESERVER)
    }

    fn default_name(&self) -> String {
        format!("Room{}", self.entity_number)
    }

    pub fn build(&self) -> MockSynapseRoom {
        self.build_fallible().unwrap()
    }
}

// The builder macro appears to confuse clippy in some way.
// For example, it thinks the build_into() methods and the entity_number fields are unused, but they aren't.
#[allow(dead_code)]
#[derive(derive_builder::Builder, Default, Debug, Clone)]
#[builder(build_fn(name = "build_fallible"), setter(into), default)]
pub struct MockSynapseUser {
    #[builder(field(ty = "test_util::RandomId"))]
    entity_number: test_util::RandomId,

    #[builder(default = "self.default_user_id()")]
    pub matrix_user_id: String,
    #[builder(default = "uuid::Uuid::new_v4().into()")]
    pub source_user_id: String,
    #[builder(default = false)]
    pub locked: bool,
}

impl MockSynapseUserBuilder {
    fn default_user_id(&self) -> String {
        format!("@user{}:{}", self.entity_number, constants::DEFAULT_MATRIX_HOMESERVER)
    }

    pub fn build(&self) -> MockSynapseUser {
        self.build_fallible().unwrap()
    }
}

pub struct SynapseApiMocker {
    pub api_mock: MockSynapseApi,
    pub synapse_rooms: Vec<MockSynapseRoom>,
    pub synapse_users: Vec<MockSynapseUser>,
}

impl SynapseApiMocker {
    pub fn new() -> Self {
        SynapseApiMocker {
            api_mock: MockSynapseApi::default(),
            synapse_rooms: Vec::new(),
            synapse_users: Vec::new(),
        }
    }

    pub fn with_rooms(mut self, rooms: Vec<MockSynapseRoom>) -> Self {
        self.synapse_rooms = rooms;
        self
    }

    pub fn with_users(mut self, users: Vec<MockSynapseUser>) -> Self {
        self.synapse_users = users;
        self
    }

    pub fn can_get_homeserver_domain(mut self, homeserver_domain: impl Into<String>) -> Self {
        let homeserver_domain = homeserver_domain.into();
        self.api_mock.expect_homeserver_domain().return_const(homeserver_domain);
        self
    }

    pub fn can_get_joined_rooms_of_syncer(mut self) -> Self {
        let rooms: Vec<String> = self.synapse_rooms.iter().map(|room| room.matrix_room_id.clone()).collect();
        self.api_mock
            .expect_get_joined_rooms_of_syncer()
            .returning(move || Ok(dto::JoinedRoomsResponse { joined_rooms: rooms.clone() }));
        self
    }

    pub fn cannot_get_joined_rooms_of_syncer(mut self) -> Self {
        self.api_mock
            .expect_get_joined_rooms_of_syncer()
            .returning(|| Err(KidsError::InternalError(error::NO_CONTEXT.to_string())));
        self
    }

    pub fn can_get_joined_rooms_of_user(mut self, user: &MockSynapseUser, rooms: Vec<&MockSynapseRoom>) -> Self {
        let rooms: Vec<String> = rooms.into_iter().map(|room| room.matrix_room_id.clone()).collect();
        self.api_mock
            .expect_get_user_joined_rooms()
            .with(eq(user.matrix_user_id.clone()))
            .returning(move |_| Ok(dto::UserJoinedRoomsResponse { joined_rooms: rooms.clone() }));
        self
    }

    pub fn require_join_user_to_room(mut self, user: &MockSynapseUser, room: &MockSynapseRoom) -> Self {
        self.api_mock
            .expect_join_user_to_room()
            .with(eq(room.matrix_room_id.clone()), eq(user.matrix_user_id.clone()))
            .times(1)
            .returning(|_, _| Ok(()));
        self
    }

    pub fn require_kick_user_from_room(mut self, user: &MockSynapseUser, room: &MockSynapseRoom) -> Self {
        self.api_mock
            .expect_kick_user_from_room()
            .with(eq(room.matrix_room_id.clone()), eq(user.matrix_user_id.clone()))
            .times(1)
            .returning(|_, _| Ok(()));
        self
    }

    pub fn can_get_all_rooms_associated_source_group_id(mut self) -> Self {
        let rooms = self.synapse_rooms.clone();
        for room in rooms {
            self = self.can_get_room_associated_source_group_id_for_room(&room);
        }
        self
    }

    pub fn can_get_room_associated_source_group_id_for_room(mut self, room: &MockSynapseRoom) -> Self {
        let room_id = room.source_room_id.clone();
        self.api_mock
            .expect_get_room_associated_source_group_id()
            .with(eq(room.matrix_room_id.clone()))
            .returning(move |_| Ok(room_id.clone()));
        self
    }

    pub fn cannot_get_room_associated_source_group_id_for_room(mut self, room: &MockSynapseRoom) -> Self {
        self.api_mock
            .expect_get_room_associated_source_group_id()
            .with(eq(room.matrix_room_id.clone()))
            .returning(|_| Err(KidsError::InternalError(error::NO_CONTEXT.to_string())));
        self
    }

    pub fn can_get_room_associated_source_group_id_v1(mut self) -> Self {
        for room in self.synapse_rooms.iter() {
            let room_id = room.source_room_id.clone();
            self.api_mock
                .expect_get_room_associated_source_group_id_v1()
                .with(eq(room.matrix_room_id.clone()))
                .returning(move |_| Ok(room_id.clone()));
        }
        self
    }

    pub fn can_associate_source_group_id_to_room(mut self) -> Self {
        self.api_mock.expect_associate_source_group_id_to_room().returning(|_, _| Ok(()));
        self
    }

    pub fn can_get_users(mut self) -> Self {
        let users: Vec<dto::User> = self.synapse_users.iter().map(Self::get_user_from).collect();
        self.api_mock
            .expect_get_users()
            .returning(move || Ok(dto::AllUsersResponse { users: users.clone() }));
        self
    }

    pub fn cannot_get_users(mut self) -> Self {
        self.api_mock
            .expect_get_users()
            .returning(|| Err(KidsError::InternalError(error::NO_CONTEXT.to_string())));
        self
    }

    pub fn can_create_room(mut self) -> Self {
        self.api_mock.expect_create_room().returning(|name, path| {
            let room = MockSynapseRoomBuilder::default().name(name).alias(path).build();
            let room_id = room.matrix_room_id.clone();
            Ok(dto::RoomCreationResponse { room_id })
        });
        self
    }

    pub fn cannot_create_room(mut self) -> Self {
        self.api_mock
            .expect_create_room()
            .returning(|_, _| Err(KidsError::InternalError(error::NO_CONTEXT.to_string())));
        self
    }

    pub fn require_delete_room(mut self, matrix_room_id: String) -> Self {
        self.api_mock.expect_delete_room().with(eq(matrix_room_id)).times(1).return_once(|_| Ok(()));
        self
    }

    pub fn can_get_room_display_name_all_rooms(mut self) -> Self {
        for room in self.synapse_rooms.iter() {
            let matrix_room_id = room.matrix_room_id.clone();
            let room_name = room.name.clone();
            self.api_mock
                .expect_get_room_display_name()
                .with(eq(matrix_room_id))
                .returning(move |_| Ok(room_name.clone()));
        }
        self.api_mock.expect_get_room_display_name().returning(|matrix_room_id| {
            Err(KidsError::ApiOperationFailed(
                error::NO_CONTEXT.to_string(),
                404,
                "get_room_display_name".to_owned(),
                anyhow::anyhow!("Could not find room with matrix id '{matrix_room_id}'."),
            ))
        });
        self
    }

    pub fn require_set_room_display_name(mut self, matrix_room_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        let matrix_room_id = matrix_room_id.into();
        let display_name = display_name.into();
        self.api_mock
            .expect_set_room_display_name()
            .with(eq(matrix_room_id), eq(display_name))
            .times(1)
            .returning(|_, _| Ok(()));
        self
    }

    pub fn can_full_room_alias(mut self) -> Self {
        self.api_mock
            .expect_full_room_alias()
            .returning(|group_path| format!("#{group_path}:testing.matrix.giz.berlin"));
        self
    }

    pub fn can_get_room_canonical_alias_all_rooms(mut self) -> Self {
        for room in self.synapse_rooms.iter() {
            let matrix_room_id = room.matrix_room_id.clone();
            let room_alias = room.alias.clone();
            self.api_mock.expect_get_room_canonical_alias().with(eq(matrix_room_id)).returning(move |_| {
                Ok(dto::RoomCanonicalAliasEvent {
                    alias: room_alias.clone(),
                    alt_aliases: None,
                })
            });
        }
        self.api_mock.expect_get_room_canonical_alias().returning(|matrix_room_id| {
            Err(KidsError::ApiOperationFailed(
                error::NO_CONTEXT.to_string(),
                404,
                "get_room_canonical_alias".to_owned(),
                anyhow::anyhow!("Could not find room with matrix id '{matrix_room_id}'."),
            ))
        });
        self
    }

    pub fn require_set_room_canonical_alias(mut self, matrix_room_id: String) -> Self {
        self.api_mock
            .expect_set_room_canonical_alias()
            .with(eq(matrix_room_id), mockall::predicate::always())
            .times(1)
            .returning(move |_, _| Ok(()));
        self
    }

    pub fn require_create_room_alias(mut self, matrix_room_id: String) -> Self {
        self.api_mock
            .expect_create_room_alias()
            .with(eq(matrix_room_id), mockall::predicate::always())
            .times(1)
            .returning(|_, _| Ok(()));
        self
    }

    pub fn can_delete_room_alias_all_aliases(mut self) -> Self {
        self.api_mock.expect_delete_room_alias().returning(|_| Ok(()));
        self
    }

    pub fn can_manage_room_members<S: Into<String>>(
        mut self,
        matrix_room_id: impl Into<String>,
        syncer_user_id: impl Into<String>,
        users_in_room: impl IntoIterator<Item = S>,
        allow_syncer_to_leave_room: bool,
        user_id_fails_to_kick: Option<S>,
    ) -> Self {
        let matrix_room_id = matrix_room_id.into();
        let syncer_user_id = syncer_user_id.into();
        let user_id_fails_to_kick = user_id_fails_to_kick.map(Into::into);
        let users_in_room: std::collections::HashMap<String, serde_json::Value> = users_in_room
            .into_iter()
            .map(Into::into)
            .map(|user_id| (user_id, serde_json::Value::Null))
            .chain([(syncer_user_id.clone(), serde_json::Value::Null)])
            .collect();
        self.api_mock.expect_user_is_matrix_syncer().with(eq(syncer_user_id.clone())).return_const(true);
        self.api_mock.expect_user_is_matrix_syncer().return_const(false);
        for user in users_in_room.keys() {
            let expectation = self.api_mock.expect_kick_user_from_room().with(eq(matrix_room_id.clone()), eq(user.to_owned()));
            if *user == syncer_user_id {
                // Syncer is never kicked but rather removed using `syncer_leave_room`.
                expectation.never();
            } else if user_id_fails_to_kick.as_ref().is_some_and(|u| u == user) {
                expectation
                    .times(1)
                    .return_once(|_, _| Err(KidsError::InternalError(error::NO_CONTEXT.to_string())));
            } else {
                expectation.times(1).return_once(|_, _| Ok(()));
            }
        }
        if allow_syncer_to_leave_room {
            self.api_mock
                .expect_syncer_leave_room()
                .with(eq(matrix_room_id.clone()))
                .return_once(|_| Ok(()));
        }
        // Catch-all forbid syncer to leave rooms.
        self.api_mock
            .expect_syncer_leave_room()
            .returning(|_| Err(KidsError::InternalError(error::NO_CONTEXT.to_string())));
        self.api_mock
            .expect_get_room_joined_users()
            .with(eq(matrix_room_id.clone()))
            .return_once(|_| Ok(dto::RoomJoinedUsersResponse { joined: users_in_room }));
        self
    }

    pub fn can_get_source_user_id_for_all_matrix_users(mut self) -> Self {
        let users = self.synapse_users.clone();
        for user in users.iter() {
            self = self.can_get_source_user_id_for_matrix_user(user)
        }
        self
    }
    pub fn can_get_source_user_id_for_matrix_user(mut self, user: &MockSynapseUser) -> Self {
        let user_id = user.source_user_id.clone();
        self.api_mock
            .expect_get_source_user_id_for_matrix_user_id()
            .with(eq(user.matrix_user_id.clone()))
            .returning(move |_| Ok(user_id.clone()));
        self
    }

    pub fn cannot_get_source_user_id_for_matrix_user(mut self, user: &MockSynapseUser) -> Self {
        self.api_mock
            .expect_get_source_user_id_for_matrix_user_id()
            .with(eq(user.matrix_user_id.clone()))
            .returning(|_| Err(KidsError::InternalError(error::NO_CONTEXT.to_string())));
        self
    }

    pub fn require_lock_user(mut self, user_to_be_locked: &MockSynapseUser) -> Self {
        self.api_mock
            .expect_lock_user()
            .with(eq(user_to_be_locked.matrix_user_id.clone()))
            .times(1)
            .return_once(|_| Ok(()));
        self
    }

    pub fn require_unlock_user(mut self, user_to_be_locked: &MockSynapseUser) -> Self {
        self.api_mock
            .expect_unlock_user()
            .with(eq(user_to_be_locked.matrix_user_id.clone()))
            .times(1)
            .return_once(|_| Ok(()));
        self
    }

    pub fn require_deactivate_user(mut self, user_to_be_locked: &MockSynapseUser) -> Self {
        self.api_mock
            .expect_deactivate_user()
            .with(eq(user_to_be_locked.matrix_user_id.clone()))
            .times(1)
            .return_once(|_| Ok(()));
        self
    }

    pub fn can_get_user_display_name(mut self, user: &MockSynapseUser, current_display_name: Option<String>) -> Self {
        self.api_mock
            .expect_get_user_display_name()
            .with(eq(user.matrix_user_id.clone()))
            .returning(move |_| Ok(current_display_name.clone()));
        self
    }

    pub fn require_set_user_display_name(mut self, user_to_be_modified: &MockSynapseUser, new_display_name: &str) -> Self {
        self.api_mock
            .expect_set_user_display_name()
            .with(eq(user_to_be_modified.matrix_user_id.clone()), eq(new_display_name.to_owned()))
            .times(1)
            .return_once(|_, _| Ok(()));
        self
    }

    pub fn can_get_user_three_pids(mut self, user: &MockSynapseUser, current_email: Option<String>) -> Self {
        self.api_mock
            .expect_get_user_three_pids()
            .with(eq(user.matrix_user_id.clone()))
            .returning(move |_| {
                Ok(if let Some(email) = current_email.as_ref() {
                    vec![dto::ThreePID {
                        medium: dto::ThreePIDMedium::Email,
                        address: email.to_owned(),
                    }]
                } else {
                    vec![]
                })
            });
        self
    }

    pub fn require_set_user_three_pids(mut self, user_to_be_modified: &MockSynapseUser, new_email: &str) -> Self {
        self.api_mock
            .expect_set_user_three_pids()
            .with(
                eq(user_to_be_modified.matrix_user_id.clone()),
                eq(vec![dto::ThreePID {
                    medium: dto::ThreePIDMedium::Email,
                    address: new_email.to_owned(),
                }]),
            )
            .times(1)
            .return_once(|_, _| Ok(()));
        self
    }

    pub fn require_create_user(mut self, matrix_user: MockSynapseUser) -> Self {
        self.api_mock
            .expect_create_user()
            .with(eq(matrix_user.matrix_user_id.clone()), eq(matrix_user.source_user_id.clone()))
            .times(1)
            .return_once(move |_, _| Ok(SynapseApiMocker::get_user_from(&matrix_user)));
        self
    }
}

impl SynapseApiMocker {
    pub fn get_user_from(user: &MockSynapseUser) -> dto::User {
        dto::User {
            name: user.matrix_user_id.clone(),
            locked: user.locked,
            external_ids: Some(vec![dto::ExternalId {
                auth_provider: constants::DEFAULT_AUTH_PROVIDER.to_string(),
                external_id: user.source_user_id.clone(),
            }]),
            threepids: None,
        }
    }
}

impl From<SynapseApiMocker> for Box<dyn external::SynapseApi + Send + Sync> {
    fn from(val: SynapseApiMocker) -> Self {
        Box::new(val.api_mock)
    }
}
