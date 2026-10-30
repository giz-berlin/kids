use anyhow::anyhow;

use kids_lib::error::KidsError;

#[derive(serde::Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct KeycloakApiConfig {
    /// Address of the external Keycloak to fetch data from.
    pub keycloak_address: String,
    /// Client ID of a Keycloak service account used to fetch the data.
    /// The service account needs to have access to the realm and have the "view-users" and "view-clients" roles assigned.
    /// Otherwise, the API will report an authentication error as it is not able to access the desired information,
    /// even when the credentials are valid.
    pub client_id: String,
    /// Keycloak client secret belonging to the `client_id`.
    pub client_secret: String,
    /// Keycloak realm to fetch the data from.
    pub realm: String,
    /// Whether service accounts should be fetched and handled like normal users.
    pub fetch_service_accounts: bool,
    /// Whether to validate the server certificate of the external API.
    /// Only disable for local development purposes!
    pub insecure_disable_tls_verification: bool,
}

/// Abstraction of the external Keycloak API, reduced to the set of methods and parameters required for this library.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait KeycloakApi: Send + Sync {
    fn client_id(&self) -> &str;
    async fn get_users(&self) -> Result<keycloak::types::TypeVec<keycloak::types::UserRepresentation>, KidsError>;
    async fn get_user(&self, user_id: &str) -> Result<keycloak::types::UserRepresentation, KidsError>;
    async fn get_user_client_roles(&self, user_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::RoleRepresentation>, KidsError>;
    async fn get_users_of_group(&self, group_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::UserRepresentation>, KidsError>;
    async fn get_groups_of_user(&self, user_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, KidsError>;
    async fn get_groups(&self) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, KidsError>;
    async fn get_subgroups(&self, group_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, KidsError>;
    async fn get_group(&self, group_id: &str) -> Result<keycloak::types::GroupRepresentation, KidsError>;
}

/// A Keycloak service account client capable of making HTTP requests to an external Keycloak instance.
/// Primary purpose is to implement the [KeycloakApi] trait.
pub struct KeycloakServiceAccountClient {
    pub config: KeycloakApiConfig,
    pub keycloak_admin: keycloak::KeycloakAdmin<OidcKeycloakTokenSupplier>,
}

pub struct OidcKeycloakTokenSupplier {
    keycloak_base_url: String,
    keycloak_accounts: oidc_rp::account::Account<
        oidc_rp::oidc::EmptyAdditionalClaims,
        oidc_rp::oidc::EmptyAdditionalClaims,
        oidc_rp::oidc::EmptyAdditionalProviderMetadata,
        oidc_rp::account::access_token_type::Opaque,
        oidc_rp::account::account_user_type::ServiceAccount,
        oidc_rp::types::AttributeSet,
        oidc_rp::types::AttributeSet,
    >,
}

fn error_to_kids_error(err: impl Into<anyhow::Error>) -> kids_lib::error::KidsError {
    let err = err.into();
    err.into()
}

/// This function enforces that an URL is interpreted as if it had at least one trailing slash.
///
/// This is useful when concatenating URL segments using [`Url::join`](oidc_rp::oidc::url::Url::join),
/// especially when user-provided segments are at play where it is unclear whether they have a trailing slash.
///
/// In other words:
///
/// * If the `url` has at least one trailing slash, nothing happens.
/// * If the `url` has no trailing slash, one is added.
fn enforce_trailing_slash(mut url: oidc_rp::oidc::url::Url) -> Result<oidc_rp::oidc::url::Url, KidsError> {
    let success = match url.path_segments_mut() {
        Ok(mut segments) => {
            // Idea from https://users.rust-lang.org/t/what-would-be-the-proper-way-to-fix-extra-slashes-in-urls/98810/5
            segments.pop_if_empty().push("");
            true
        }
        // The `url` is cannot-be-a-base.
        Err(()) => false,
    };
    // This hoop is required due to the `Drop` impl of `PathSegmentsMut`:
    // Using `url` before even the `Result` is dropped is impossible,
    // even in the `Err` case.
    if !success {
        return Err(KidsError::InternalError(anyhow!("The url '{url}' is cannot-be-a-base.")));
    }
    Ok(url)
}

impl OidcKeycloakTokenSupplier {
    async fn new(client_id: &str, client_secret: &str, realm: &str, http_client: reqwest::Client, keycloak_base_url: &str) -> Result<Self, KidsError> {
        let idp = oidc_rp::idp::IdP::new_with_reqwest_client(
            enforce_trailing_slash(keycloak_base_url.parse().map_err(error_to_kids_error)?)?
                .join("realms/")
                .map_err(error_to_kids_error)?
                .join(realm)
                .map_err(error_to_kids_error)?,
            http_client,
        )
        .await
        .map_err(error_to_kids_error)?
        .set_default_idp_refresh_strategy()
        .await
        .map_err(error_to_kids_error)?;
        let verifier = oidc_rp::verifier::Verifier::<oidc_rp::oidc::EmptyAdditionalClaims>::new(idp.clone(), client_id.to_owned())
            .map_err(error_to_kids_error)?
            .set_access_token_allowed_jose_types(vec![
                oidc_rp::oidc::JsonWebTokenType::new("JWT".to_owned())
                    .normalize()
                    .map_err(error_to_kids_error)?,
            ])
            .allow_other_audiences();
        let account = oidc_rp::account::Account::from_secret_client(idp, client_id.to_owned(), client_secret.to_owned(), verifier);
        let account = account.exchange_client_credentials().await.map_err(error_to_kids_error)?;
        Ok(Self {
            keycloak_accounts: account.start_auto_refresh(),
            keycloak_base_url: keycloak_base_url.to_owned(),
        })
        // todo!()
    }
}
impl keycloak::KeycloakTokenSupplier for OidcKeycloakTokenSupplier {
    fn get<'life0, 'life1, 'async_trait>(
        &'life0 self,
        url: &'life1 str,
    ) -> ::core::pin::Pin<Box<dyn ::core::future::Future<Output = Result<String, keycloak::KeycloakError>> + ::core::marker::Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let keycloak_base_url = url;
            if keycloak_base_url != self.keycloak_base_url {
                panic!("Can only serve keycloak base URL {}, requested {keycloak_base_url}.", self.keycloak_base_url);
            }
            Ok(self.keycloak_accounts.get_access_token().await.expect("Cannot get token"))
        })
    }
}

impl KeycloakServiceAccountClient {
    pub async fn new(config: KeycloakApiConfig) -> Result<Self, KidsError> {
        if config.insecure_disable_tls_verification {
            tracing::warn!("Verification of Keycloak server certificate is disabled. Do not use this setting in a production environment!");
        }

        let token_retriever_http_client = reqwest::Client::builder()
            .danger_accept_invalid_certs(config.insecure_disable_tls_verification)
            .build()
            .unwrap();
        let keycloak_client = OidcKeycloakTokenSupplier::new(
            config.client_id.as_ref(),
            config.client_secret.as_ref(),
            config.realm.as_ref(),
            token_retriever_http_client,
            config.keycloak_address.as_ref(),
        )
        .await?;

        let api_http_client = reqwest::Client::builder()
            .danger_accept_invalid_certs(config.insecure_disable_tls_verification)
            .build()
            .unwrap();
        let keycloak_admin = keycloak::KeycloakAdmin::new(&config.keycloak_address, keycloak_client, api_http_client);

        Ok(KeycloakServiceAccountClient { config, keycloak_admin })
    }

    fn convert_error<Resource>(route: &str, data: Result<Resource, keycloak::KeycloakError>) -> Result<Resource, KidsError> {
        match data {
            Ok(resource) => Ok(resource),
            Err(keycloak::KeycloakError::HttpFailure { status, text, .. }) => {
                if status == 401 || status == 403 {
                    return Err(KidsError::AuthenticationFailed(
                        kids_lib::error::no_context(),
                        status,
                        route.to_string(),
                        anyhow!(text),
                    ));
                }
                Err(KidsError::ApiOperationFailed(
                    kids_lib::error::no_context(),
                    status,
                    route.to_string(),
                    anyhow!(text),
                ))
            }
            Err(e) => {
                tracing::error!(error = ?e, "Unknown Keycloak error");
                Err(KidsError::RequestFailed(kids_lib::error::no_context(), anyhow!(e)))
            }
        }
    }
}

/// As we do not support pagination, we need to make we always receive the full list of
/// [UserRepresentations](keycloak::types::UserRepresentation) and
/// [GroupRepresentations](keycloak::types::GroupRepresentation) from Keycloak
/// with any of the [KeycloakApi] calls.
///
/// If we were to miss some entries, this would cause them to
/// be deleted in the configured [Target](crate::interface::target::Target).
const FETCH_ALL_ENTITIES: i32 = -1;

#[async_trait::async_trait]
impl KeycloakApi for KeycloakServiceAccountClient {
    fn client_id(&self) -> &str {
        &self.config.client_id
    }

    async fn get_users(&self) -> Result<keycloak::types::TypeVec<keycloak::types::UserRepresentation>, KidsError> {
        KeycloakServiceAccountClient::convert_error(
            "GET_USERS",
            self.keycloak_admin
                .realm(&self.config.realm)
                .users_get()
                // This sets the query parameter `exact` if service accounts should be included. The value does not matter, it could also be `false`.
                // The parameter does not filter the users if other attributes such as components of the name are not given. However, because a
                // "filtering" attribute is set, Keycloak handles the request differently and includes service accounts in the result.
                // This is hacky and can hopefully be replaced when github.com/keycloak/keycloak/pull/51788 is merged.
                .exact(if self.config.fetch_service_accounts { Some(true) } else { None })
                .max(FETCH_ALL_ENTITIES)
                .await,
        )
    }

    async fn get_user(&self, user_id: &str) -> Result<keycloak::types::UserRepresentation, KidsError> {
        KeycloakServiceAccountClient::convert_error("GET_USER", self.keycloak_admin.realm(&self.config.realm).users_with_user_id_get(user_id).await)
    }

    async fn get_user_client_roles(&self, user_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::RoleRepresentation>, KidsError> {
        let client_uuid = {
            let clients = KeycloakServiceAccountClient::convert_error(
                "GET_REALM_CLIENTS",
                self.keycloak_admin
                    .realm(&self.config.realm)
                    .clients_get()
                    .client_id(self.config.client_id.clone())
                    .max(FETCH_ALL_ENTITIES)
                    .search(true)
                    .await,
            )?;
            let client_uuid = match clients.len() {
                1 => clients.into_iter().next().expect("We have just ensured that there is one element").id,
                0 => {
                    return Err(KidsError::InternalError(anyhow!(
                        "Could not find client with clientId {}.",
                        self.config.client_id
                    )));
                }
                len => {
                    return Err(KidsError::InternalError(anyhow!(
                        "Search for client with clientId {} returned {} results",
                        self.config.client_id,
                        len
                    )));
                }
            };
            match client_uuid {
                Some(client_uuid) => client_uuid,
                None => {
                    return Err(KidsError::InternalError(anyhow!(
                        "Could not find client id for client with clientId {}.",
                        self.config.client_id
                    )));
                }
            }
        };
        KeycloakServiceAccountClient::convert_error(
            "GET_USER_CLIENT_ROLES",
            self.keycloak_admin
                .realm(&self.config.realm)
                .users_with_user_id_role_mappings_clients_with_client_id_composite_get(user_id, client_uuid.as_ref())
                .brief_representation(true)
                .await,
        )
    }

    async fn get_users_of_group(&self, group_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::UserRepresentation>, KidsError> {
        KeycloakServiceAccountClient::convert_error(
            "GET_USERS_OF_GROUP",
            self.keycloak_admin
                .realm(&self.config.realm)
                .groups_with_group_id_members_get(group_id)
                .brief_representation(false)
                .max(FETCH_ALL_ENTITIES)
                .await,
        )
    }

    async fn get_groups_of_user(&self, user_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, KidsError> {
        KeycloakServiceAccountClient::convert_error(
            "GET_GROUPS_OF_USERS",
            self.keycloak_admin
                .realm(&self.config.realm)
                .users_with_user_id_groups_get(user_id)
                .brief_representation(false)
                .max(FETCH_ALL_ENTITIES)
                .await,
        )
    }

    async fn get_groups(&self) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, KidsError> {
        KeycloakServiceAccountClient::convert_error(
            "GET_GROUPS",
            self.keycloak_admin
                .realm(&self.config.realm)
                .groups_get()
                .brief_representation(false)
                .max(FETCH_ALL_ENTITIES)
                .await,
        )
    }

    async fn get_subgroups(&self, group_id: &str) -> Result<keycloak::types::TypeVec<keycloak::types::GroupRepresentation>, KidsError> {
        KeycloakServiceAccountClient::convert_error(
            "GET_SUBGROUPS",
            self.keycloak_admin
                .realm(&self.config.realm)
                .groups_with_group_id_children_get(group_id)
                .brief_representation(false)
                .max(FETCH_ALL_ENTITIES)
                .await,
        )
    }

    async fn get_group(&self, group_id: &str) -> Result<keycloak::types::GroupRepresentation, KidsError> {
        KeycloakServiceAccountClient::convert_error(
            "GET_GROUP",
            self.keycloak_admin.realm(&self.config.realm).groups_with_group_id_get(group_id).await,
        )
    }
}

// The builder macro appears to confuse clippy in some way.
// For example, it thinks the build_into() methods and the entity_number fields are unused, but they aren't.
#[allow(dead_code)]
#[cfg(test)]
pub mod test {
    use std::collections::HashMap;

    pub fn default_keycloak_config(email_attribute: Option<&str>) -> std::sync::Arc<crate::connector::KeycloakConfig> {
        std::sync::Arc::new(crate::connector::KeycloakConfig {
            keycloak_api: super::KeycloakApiConfig {
                keycloak_address: "http://localhost".to_string(),
                client_id: "client".to_string(),
                client_secret: "secret".to_string(),
                realm: "realm".to_string(),
                fetch_service_accounts: false,
                insecure_disable_tls_verification: false,
            },
            email_attribute: email_attribute.map(str::to_string),
        })
    }

    #[derive(derive_builder::Builder, Default, Debug)]
    #[builder(setter(into), default)]
    pub struct KeycloakUserRepresentation {
        #[builder(field(ty = "kids_test_lib::util::RandomId"))]
        entity_number: kids_test_lib::util::RandomId,

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
            format!("user_{}", self.entity_number)
        }

        fn default_email(&self) -> String {
            format!("user_{}@test.giz.berlin", self.entity_number)
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
        #[builder(field(ty = "kids_test_lib::util::RandomId"))]
        entity_number: kids_test_lib::util::RandomId,

        #[builder(default = "uuid::Uuid::new_v4().into()")]
        id: String,
        #[builder(default = "self.default_name()")]
        name: String,
        #[builder(default = "self.default_path()")]
        path: String,
        #[builder(setter(into, strip_option))]
        parent_id: Option<String>,
    }

    impl KeycloakGroupRepresentationBuilder {
        fn default_name(&self) -> String {
            format!("Group_{}", self.entity_number)
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
                parent_id: value.parent_id,
                ..Default::default()
            }
        }
    }

    mod enforce_trailing_slash {
        use super::super::{KidsError, enforce_trailing_slash};

        fn url(url: impl Into<String>) -> oidc_rp::oidc::url::Url {
            url.into().parse().unwrap()
        }

        #[test]
        fn trailing_slashes() {
            assert_eq!(enforce_trailing_slash(url("http://localhost/a")).unwrap(), url("http://localhost/a/"));
            assert_eq!(enforce_trailing_slash(url("http://localhost/a/")).unwrap(), url("http://localhost/a/"));
            assert_eq!(enforce_trailing_slash(url("http://localhost/a//")).unwrap(), url("http://localhost/a//"));
        }

        #[test]
        fn cannot_be_a_base() {
            assert!(matches!(
                enforce_trailing_slash(url("mailto:me@example.com")).unwrap_err(),
                KidsError::InternalError(err) if err.to_string() == "The url 'mailto:me@example.com' is cannot-be-a-base."
            ));
        }
    }
}
