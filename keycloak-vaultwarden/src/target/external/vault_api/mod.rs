mod dto;

pub use dto::{Group, Member};

use kids_lib::error::KidsError;
use kids_lib::types::ApiPath;

use crate::target::external::http::auth::{ApiKey, Scope, TokenAuthenticator};
use crate::target::external::http::{ApiClient, HttpConfig};
use crate::target::types::Access;

pub struct VaultApiClient {
    org_id: String,
    transport: ApiClient<TokenAuthenticator>,
}

impl VaultApiClient {
    pub async fn new(http: &HttpConfig, org_id: String, user_api_key: ApiKey) -> Result<Self, KidsError> {
        let transport = ApiClient::new(http, "api", TokenAuthenticator::new(user_api_key, Scope::User)).await?;
        Ok(Self { org_id, transport })
    }

    /// Email of the account the syncer is logged in as.
    pub async fn get_own_email(&self) -> Result<String, KidsError> {
        let profile: dto::Profile = self
            .transport
            .request(http::Method::GET, ApiPath::from_segments(["accounts", "profile"]), None::<()>)
            .await
            .map_err(|error| error.with_context("Fetching own profile"))?;
        Ok(profile.email)
    }

    pub async fn get_members(&self) -> Result<Vec<Member>, KidsError> {
        let response: dto::ListResponse<Member> = self
            .transport
            .request(http::Method::GET, ApiPath::from_segments(["organizations", &self.org_id, "users"]), None::<()>)
            .await
            .map_err(|error| error.with_context("Fetching members"))?;
        Ok(response.data)
    }

    pub async fn get_groups(&self) -> Result<Vec<Group>, KidsError> {
        let response: dto::ListResponse<Group> = self
            .transport
            .request(
                http::Method::GET,
                ApiPath::from_segments(["organizations", &self.org_id, "groups", "details"]),
                None::<()>,
            )
            .await
            .map_err(|error| error.with_context("Fetching groups"))?;
        Ok(response.data)
    }

    pub async fn get_group_member_ids(&self, group_id: &str) -> Result<Vec<String>, KidsError> {
        self.transport
            .request(
                http::Method::GET,
                ApiPath::from_segments(["organizations", &self.org_id, "groups", group_id, "users"]),
                None::<()>,
            )
            .await
            .map_err(|error| error.with_context(&format!("Fetching members of group {group_id}")))
    }

    pub async fn create_group(&self, name: &str, external_id: &str) -> Result<String, KidsError> {
        let response: dto::CreatedGroup = self
            .transport
            .request(
                http::Method::POST,
                ApiPath::from_segments(["organizations", &self.org_id, "groups"]),
                Some(dto::GroupRequest {
                    name,
                    access_all: false,
                    external_id: Some(external_id),
                    collections: &[],
                    users: &[],
                }),
            )
            .await
            .map_err(|error| error.with_context(&format!("Creating group '{name}'")))?;
        tracing::info!(name, external_id, group_id = response.id, "Created group");
        Ok(response.id)
    }

    /// Replaces the group's name, `accessAll` flag, collection access and members.
    pub async fn update_group(&self, group_id: &str, name: &str, access_all: bool, collections: &[Access], member_ids: &[String]) -> Result<(), KidsError> {
        self.transport
            .request_discarding_response(
                http::Method::PUT,
                ApiPath::from_segments(["organizations", &self.org_id, "groups", group_id]),
                // Vaultwarden ignores the external ID on updates, it can only be set on creation.
                Some(dto::GroupRequest {
                    name,
                    access_all,
                    external_id: None,
                    collections,
                    users: member_ids,
                }),
                &[],
            )
            .await
            .map_err(|error| error.with_context(&format!("Updating group {group_id}")))?;
        tracing::info!(group_id, name, "Updated group");
        Ok(())
    }

    /// Deletes a group.
    /// Also succeeds if the group does not exist.
    pub async fn delete_group(&self, group_id: &str) -> Result<(), KidsError> {
        let result = self
            .transport
            .request_discarding_response(
                http::Method::DELETE,
                ApiPath::from_segments(["organizations", &self.org_id, "groups", group_id]),
                None::<()>,
                &[],
            )
            .await;
        if let Err(error) = result {
            // Vaultwarden answers `400 Bad Request` for a missing group, like for any other invalid request.
            if self.get_groups().await?.iter().any(|group| group.id == group_id) {
                return Err(error.with_context(&format!("Deleting group {group_id}")));
            }
            tracing::info!(group_id, "Group was already deleted");
            return Ok(());
        }
        tracing::info!(group_id, "Deleted group");
        Ok(())
    }

    /// Deletes a collection. Its items stay in the organization, unassigned to any collection.
    /// Also succeeds if the collection does not exist.
    pub async fn delete_collection(&self, collection_id: &str) -> Result<(), KidsError> {
        let result = self
            .transport
            .request_discarding_response(
                http::Method::DELETE,
                ApiPath::from_segments(["organizations", &self.org_id, "collections", collection_id]),
                None::<()>,
                &[],
            )
            .await;
        if let Err(error) = result {
            // Vaultwarden answers `401 Unauthorized` for a missing collection, as its permission check
            // finds no collection the syncer may manage.
            if self.get_collection_ids().await?.iter().any(|id| id == collection_id) {
                return Err(error.with_context(&format!("Deleting collection {collection_id}")));
            }
            tracing::info!(collection_id, "Collection was already deleted");
            return Ok(());
        }
        tracing::info!(collection_id, "Deleted collection");
        Ok(())
    }

    async fn get_collection_ids(&self) -> Result<Vec<String>, KidsError> {
        let response: dto::ListResponse<dto::Collection> = self
            .transport
            .request(
                http::Method::GET,
                ApiPath::from_segments(["organizations", &self.org_id, "collections"]),
                None::<()>,
            )
            .await
            .map_err(|error| error.with_context("Fetching collections"))?;
        Ok(response.data.into_iter().map(|collection| collection.id).collect())
    }

    /// Members with direct access to the collection, not through a group.
    pub async fn get_collection_member_access(&self, collection_id: &str) -> Result<Vec<Access>, KidsError> {
        self.transport
            .request(
                http::Method::GET,
                ApiPath::from_segments(["organizations", &self.org_id, "collections", collection_id, "users"]),
                None::<()>,
            )
            .await
            .map_err(|error| error.with_context(&format!("Fetching member access of collection {collection_id}")))
    }
}
