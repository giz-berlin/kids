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
        }
    }
}

impl From<SynapseApiMocker> for Box<dyn external::SynapseApi + Send + Sync> {
    fn from(val: SynapseApiMocker) -> Self {
        Box::new(val.api_mock)
    }
}
