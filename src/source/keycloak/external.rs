use crate::error;

#[derive(serde::Deserialize)]
pub struct KeycloakConfig {
    /// Adress of external Keycloak to fetch data from.
    pub keycloak_address: String,
    /// Client ID of a Keycloak service account used to fetch the data.
    /// The service account needs to have access to the realm and have the "view-users" and "view-realm" roles assigned.
    /// Otherwise, the API will report an authentication error as it is not able to access the desired information,
    /// even the credentials are valid.
    pub client_id: String,
    /// Keycloak client secret belonging to the `client_id`.
    pub client_secret: String,
    /// Keycloak realm to fetch the data from.
    pub realm: String,
    /// Number of [UserRepresentations](keycloak::types::UserRepresentation) or
    /// [GroupRepresentations](keycloak::types::GroupRepresentation) to fetch from Keycloak
    /// with any of the [KeycloakApi] calls.
    /// As we do not support pagination, this value must be high enough to **guarantee** that we
    /// will receive the full list, or else we might miss some entities, which would cause them to
    /// be deleted in the configured [Target](crate::target::interface::Target).
    pub num_entities_to_fetch: i32,
}

/// Abstraction of the external keycloak API, reduced to the set of methods and parameters required for this library.
#[mockall::automock]
#[async_trait::async_trait(?Send)]
pub trait KeycloakApi {
    async fn get_users(&self) -> Result<keycloak::types::TypeVec<keycloak::types::UserRepresentation>, error::KidsError>;
    async fn get_groups_of_user(&self, user_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, error::KidsError>;
    async fn get_groups(&self) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, error::KidsError>;
    async fn get_subgroups(&self, group_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, error::KidsError>;
}

/// A Keycloak service account client capable of making HTTP requests to an external Keycloak instance.
/// Primary purpose is to implement the [KeycloakApi] trait.
pub struct KeycloakServiceAccountClient {
    pub config: KeycloakConfig,
    pub keycloak_admin: keycloak::KeycloakAdmin<keycloak::KeycloakServiceAccountAdminTokenRetriever>,
}

impl KeycloakServiceAccountClient {
    pub fn new(config: KeycloakConfig) -> Self {
        let keycloak_client = keycloak::KeycloakServiceAccountAdminTokenRetriever::create_with_custom_realm(
            &config.client_id,
            &config.client_secret,
            &config.realm,
            reqwest::Client::new(),
        );
        let keycloak_admin = keycloak::KeycloakAdmin::new(&config.keycloak_address, keycloak_client, reqwest::Client::new());

        KeycloakServiceAccountClient { config, keycloak_admin }
    }

    fn convert_error<Resource>(
        data: Result<keycloak::types::TypeVec<Resource>, keycloak::KeycloakError>,
    ) -> Result<keycloak::types::TypeVec<Resource>, error::KidsError> {
        match data {
            Ok(resource) => Ok(resource),
            Err(keycloak::KeycloakError::HttpFailure { status, .. }) => {
                if status == 401 || status == 403 {
                    return Err(error::KidsError::AuthenticationFailure);
                }
                Err(error::KidsError::HttpFailure(status))
            }
            Err(_) => Err(error::KidsError::RequestFailure),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl KeycloakApi for KeycloakServiceAccountClient {
    async fn get_users(&self) -> Result<keycloak::types::TypeVec<keycloak::types::UserRepresentation>, error::KidsError> {
        KeycloakServiceAccountClient::convert_error(
            self.keycloak_admin
                .realm_users_get(
                    &self.config.realm,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(self.config.num_entities_to_fetch),
                    None,
                    None,
                    None,
                )
                .await,
        )
    }

    async fn get_groups_of_user(&self, user_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, error::KidsError> {
        KeycloakServiceAccountClient::convert_error(
            self.keycloak_admin
                .realm_users_with_user_id_groups_get(&self.config.realm, user_id, None, None, Some(self.config.num_entities_to_fetch), None)
                .await,
        )
    }

    async fn get_groups(&self) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, error::KidsError> {
        KeycloakServiceAccountClient::convert_error(
            self.keycloak_admin
                .realm_groups_get(&self.config.realm, None, None, None, Some(self.config.num_entities_to_fetch), None, None, None)
                .await,
        )
    }

    async fn get_subgroups(&self, group_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, error::KidsError> {
        KeycloakServiceAccountClient::convert_error(
            self.keycloak_admin
                .realm_groups_with_group_id_children_get(&self.config.realm, group_id, None, None, None, Some(self.config.num_entities_to_fetch), None)
                .await,
        )
    }
}

// The builder macro appears to confuse clippy in some way.
// For example, it thinks the build_into() methods and the entity_number fields are unused, but they aren't.
#[allow(dead_code)]
pub mod test {
    use crate::util;
    use std::collections::HashMap;

    #[derive(derive_builder::Builder, Default, Debug)]
    #[builder(setter(into), default)]
    pub struct KeycloakUserRepresentation {
        #[builder(field(ty = "util::RandomId"))]
        entity_number: util::RandomId,

        #[builder(default = "uuid::Uuid::new_v4().into()")]
        id: String,
        #[builder(default = "true")]
        enabled: bool,
        #[builder(default = "self.default_username()")]
        username: String,
        #[builder(default = "self.default_email()")]
        email: String,
        #[builder(setter(each(name = "attr")))]
        attributes: HashMap<String, Vec<String>>,
        #[builder(setter(each(name = "role")))]
        roles: Vec<String>,
    }

    impl KeycloakUserRepresentationBuilder {
        pub fn attribute(&mut self, key: &str, value: &str) -> &mut Self {
            self.attr((key.to_owned(), vec![value.to_owned()]));
            self
        }

        fn default_username(&self) -> String {
            format!("user_{}", &self.entity_number)
        }

        fn default_email(&self) -> String {
            format!("user_{}@test.giz.berlin", &self.entity_number)
        }

        pub fn build_into(&self) -> keycloak::types::UserRepresentation {
            self.build().unwrap().into()
        }
    }

    impl From<KeycloakUserRepresentation> for keycloak::types::UserRepresentation {
        fn from(value: KeycloakUserRepresentation) -> Self {
            keycloak::types::UserRepresentation {
                id: Some(value.id),
                enabled: Some(value.enabled),
                username: Some(value.username),
                email: Some(value.email),
                attributes: Some(value.attributes),
                realm_roles: Some(value.roles),
                ..Default::default()
            }
        }
    }

    #[derive(derive_builder::Builder, Default, Debug)]
    #[builder(setter(into), default)]
    pub struct KeycloakGroupRepresentation {
        #[builder(field(ty = "util::RandomId"))]
        entity_number: util::RandomId,

        #[builder(default = "uuid::Uuid::new_v4().into()")]
        id: String,
        #[builder(default = "self.default_name()")]
        name: String,
        #[builder(default = "self.default_path()")]
        path: String,
    }

    impl KeycloakGroupRepresentationBuilder {
        fn default_name(&self) -> String {
            format!("Group_{}", &self.entity_number)
        }

        fn default_path(&self) -> String {
            format!("/{}", self.default_name())
        }

        pub fn build_into(&self) -> keycloak::types::GroupRepresentation {
            self.build().unwrap().into()
        }
    }

    impl From<KeycloakGroupRepresentation> for keycloak::types::GroupRepresentation {
        fn from(value: KeycloakGroupRepresentation) -> Self {
            keycloak::types::GroupRepresentation {
                id: Some(value.id),
                name: Some(value.name),
                path: Some(value.path),
                ..Default::default()
            }
        }
    }
}
