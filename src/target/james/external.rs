use crate::error;
use crate::target::james::dto;
use anyhow::anyhow;
use rand::distr::{Alphanumeric, SampleString};
use reqwest::RequestBuilder;
use urlencoding;

#[derive(serde::Deserialize, Clone)]
pub struct JamesApiConfig {
    pub james_user_domain: String,
    pub james_list_domain: String,
    pub james_team_domain: String,
    pub base_url: String,
}

#[async_trait::async_trait(?Send)]
pub trait JamesApi {
    async fn get_users(&mut self) -> Result<Vec<dto::User>, error::KidsError>;
    async fn create_user(&mut self, user_email: &str) -> Result<(), error::KidsError>;
    async fn delete_user(&mut self, user_email: &str) -> Result<(), error::KidsError>;
    async fn create_mailbox(&mut self, user_email: &str, mailbox_name: &str) -> Result<(), error::KidsError>;
    async fn delete_mailbox(&mut self, user_email: &str, mailbox_name: &str) -> Result<(), error::KidsError>;
    async fn get_lists(&mut self) -> Result<Vec<String>, error::KidsError>;
    async fn get_list_members(&mut self, list_email: &str) -> Result<Vec<String>, error::KidsError>;
    async fn add_member_to_list(&mut self, list_email: &str, user_email: &str) -> Result<(), error::KidsError>;
    async fn remove_member_from_list(&mut self, list_email: &str, user_email: &str) -> Result<(), error::KidsError>;
    async fn get_aliases_of(&mut self, email: &str) -> Result<Vec<dto::Alias>, error::KidsError>;
    async fn add_alias(&mut self, email: &str, alias_email: &str) -> Result<(), error::KidsError>;
    async fn remove_alias(&mut self, email: &str, alias_email: &str) -> Result<(), error::KidsError>;
    async fn get_teams(&mut self) -> Result<Vec<dto::Team>, error::KidsError>;
    async fn create_team(&mut self, team_id: &str) -> Result<(), error::KidsError>;
    async fn delete_team(&mut self, team_id: &str) -> Result<(), error::KidsError>;
    async fn get_team_members(&mut self, team_id: &str) -> Result<Vec<dto::Member>, error::KidsError>;
    async fn add_member_to_team(&mut self, team_id: &str, user_email: &str) -> Result<(), error::KidsError>;
    async fn remove_member_from_team(&mut self, team_id: &str, user_email: &str) -> Result<(), error::KidsError>;
    async fn get_user_teams(&mut self, user_email: &str) -> Result<Vec<dto::Team>, error::KidsError>;

    async fn get_domains(&mut self) -> Result<Vec<String>, error::KidsError>;
}

pub struct JamesClient {
    config: JamesApiConfig,
    http_client: reqwest::Client,
    parsed_base_url: url::Url,
}

impl JamesClient {
    pub async fn new(config: JamesApiConfig) -> Result<Self, error::KidsError> {
        let parsed_base_url = url::Url::parse(&config.base_url).expect("Base URL should be parseable");
        tracing::info!(base_url=%parsed_base_url, "Connecting to base URL");

        let builder = reqwest::Client::builder();
        let client = builder.build().expect("Client should be built");

        let james_client = JamesClient {
            config,
            http_client: client,
            parsed_base_url,
        };

        Ok(james_client)
    }

    async fn send_api_get_request<T: serde::de::DeserializeOwned>(&mut self, path: String) -> Result<T, error::KidsError> {
        self.send_api_request::<(), T>(http::Method::GET, path, None).await
    }

    async fn send_api_request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &mut self,
        method: http::Method,
        path: String,
        body: Option<B>,
    ) -> Result<T, error::KidsError> {
        let joined_url = self.parsed_base_url.join(&path).expect("Path should be joinable with parsed base URL");
        let mut request = self.http_client.request(method, joined_url);
        if let Some(body) = body {
            request = request.json(&body)
        }
        self.send_request(request).await
    }

    async fn send_request<T: serde::de::DeserializeOwned>(&mut self, request: RequestBuilder) -> Result<T, error::KidsError> {
        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let url = response.url().to_string();
                if status.is_success() {
                    // James webadmin API do not return valid json which results in an error when decoding the json afterward
                    if status.as_u16() == 204 {
                        return Ok(serde_json::from_str("null").unwrap());
                    }
                    return match response.json().await {
                        Ok(json) => Ok(json),
                        Err(error) => Err(error::KidsError::ApiOperationFailed(
                            error::NO_CONTEXT.to_string(),
                            status.as_u16(),
                            url,
                            anyhow!(error),
                        )),
                    };
                }

                let error_information = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to obtain error information from response text".to_string());

                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Err(error::KidsError::AuthenticationFailed(
                        error::NO_CONTEXT.to_string(),
                        status.as_u16(),
                        url,
                        anyhow!(error_information),
                    ));
                }

                Err(error::KidsError::ApiOperationFailed(
                    error::NO_CONTEXT.to_string(),
                    status.as_u16(),
                    url,
                    anyhow!(error_information),
                ))
            }
            Err(e) => Err(error::KidsError::RequestFailed(error::NO_CONTEXT.to_string(), anyhow!(e))),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl JamesApi for JamesClient {
    async fn get_users(&mut self) -> Result<Vec<dto::User>, error::KidsError> {
        let users: Vec<dto::User> = self.send_api_get_request("users".to_owned()).await?;
        Ok(users)
    }

    async fn create_user(&mut self, user_email: &str) -> Result<(), error::KidsError> {
        let password = Alphanumeric.sample_string(&mut rand::rng(), 50);
        let _: () = self
            .send_api_request(
                http::Method::PUT,
                format!("users/{}", urlencoding::encode(user_email)),
                Some(&serde_json::json!({
                    "password": password
                })),
            )
            .await?;
        Ok(())
    }

    async fn delete_user(&mut self, user_email: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(http::Method::DELETE, format!("users/{}", urlencoding::encode(user_email)), None)
            .await?;
        Ok(())
    }

    async fn create_mailbox(&mut self, user_email: &str, mailbox_name: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::PUT,
                format!("users/{}/mailboxes/{}", urlencoding::encode(user_email), mailbox_name),
                None,
            )
            .await?;
        Ok(())
    }

    async fn delete_mailbox(&mut self, user_email: &str, mailbox_name: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::DELETE,
                format!("users/{}/mailboxes/{}", urlencoding::encode(user_email), mailbox_name),
                None,
            )
            .await?;
        Ok(())
    }

    async fn get_lists(&mut self) -> Result<Vec<String>, error::KidsError> {
        let lists: Vec<String> = self.send_api_get_request("address/groups".to_string()).await?;
        Ok(lists)
    }

    async fn get_list_members(&mut self, list_email: &str) -> Result<Vec<String>, error::KidsError> {
        let members: Vec<String> = self.send_api_get_request(format!("address/groups/{}", urlencoding::encode(list_email))).await?;
        Ok(members)
    }

    async fn add_member_to_list(&mut self, list_email: &str, user_email: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::PUT,
                format!("address/groups/{}/{}", urlencoding::encode(list_email), urlencoding::encode(user_email)),
                None,
            )
            .await?;
        Ok(())
    }

    async fn remove_member_from_list(&mut self, list_email: &str, user_email: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::DELETE,
                format!("address/groups/{}/{}", urlencoding::encode(list_email), urlencoding::encode(user_email)),
                None,
            )
            .await?;
        Ok(())
    }

    async fn get_aliases_of(&mut self, email: &str) -> Result<Vec<dto::Alias>, error::KidsError> {
        let aliases = self.send_api_get_request(format!("address/aliases/{}", urlencoding::encode(email))).await?;
        Ok(aliases)
    }

    async fn add_alias(&mut self, email: &str, alias_email: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::PUT,
                format!("address/aliases/{}/sources/{}", urlencoding::encode(email), urlencoding::encode(alias_email)),
                None,
            )
            .await?;
        Ok(())
    }

    async fn remove_alias(&mut self, email: &str, alias_email: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::DELETE,
                format!("address/aliases/{}/sources/{}", urlencoding::encode(email), urlencoding::encode(alias_email)),
                None,
            )
            .await?;
        Ok(())
    }

    async fn get_teams(&mut self) -> Result<Vec<dto::Team>, error::KidsError> {
        let teams: Vec<dto::Team> = self
            .send_api_get_request(format!("/domains/{}/team-mailboxes", self.config.james_team_domain))
            .await?;
        Ok(teams)
    }

    async fn create_team(&mut self, team_id: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::PUT,
                format!("domains/{}/team-mailboxes/{}", self.config.james_team_domain, team_id),
                None,
            )
            .await?;
        Ok(())
    }

    async fn delete_team(&mut self, team_id: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::DELETE,
                format!("domains/{}/team-mailboxes/{}", self.config.james_team_domain, team_id),
                None,
            )
            .await?;
        Ok(())
    }

    async fn get_team_members(&mut self, team_id: &str) -> Result<Vec<dto::Member>, error::KidsError> {
        let teams: Vec<dto::Member> = self
            .send_api_get_request(format!("domains/{}/team-mailboxes/{}/members", self.config.james_team_domain, team_id))
            .await?;
        Ok(teams)
    }

    async fn add_member_to_team(&mut self, team_id: &str, user_email: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::PUT,
                format!(
                    "domains/{}/team-mailboxes/{}/members/{}?role=member",
                    self.config.james_team_domain,
                    team_id,
                    urlencoding::encode(user_email)
                ),
                None,
            )
            .await?;
        Ok(())
    }

    async fn remove_member_from_team(&mut self, team_id: &str, user_email: &str) -> Result<(), error::KidsError> {
        let _ = self
            .send_api_request::<(), serde_json::Value>(
                http::Method::DELETE,
                format!(
                    "domains/{}/team-mailboxes/{}/members/{}",
                    self.config.james_team_domain,
                    team_id,
                    urlencoding::encode(user_email)
                ),
                None,
            )
            .await?;
        Ok(())
    }

    async fn get_user_teams(&mut self, user_email: &str) -> Result<Vec<dto::Team>, error::KidsError> {
        let teams: Vec<dto::Team> = self
            .send_api_get_request(format!("/users/{}/team-mailboxes", urlencoding::encode(user_email)))
            .await?;
        Ok(teams)
    }

    async fn get_domains(&mut self) -> Result<Vec<String>, error::KidsError> {
        let domains: Vec<String> = self.send_api_get_request("/domains".to_string()).await?;
        Ok(domains)
    }
}
