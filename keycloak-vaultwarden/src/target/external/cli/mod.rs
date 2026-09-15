//! Client wrapping the official `bw` CLI.
//!
//! Some operations require client-side cryptography with the organization key, which only a
//! logged-in client with an unlocked vault can do: confirming members (the organization key is
//! encrypted with the member's public key) and managing collections (their names are encrypted).

mod dto;

pub use dto::Collection;

use anyhow::anyhow;

use kids_lib::error::KidsError;

use crate::target::external::http::HttpConfig;
use crate::target::external::http::auth::ApiKey;
use crate::target::types::Access;

/// What `bw --nointeraction` prints to stderr when it is not logged in or its vault is locked.
const INVALID_SESSION_MESSAGES: [&str; 2] = ["You are not logged in", "Vault is locked"];

pub struct CliClient {
    http: HttpConfig,
    org_id: String,
    user_api_key: ApiKey,
    master_password: String,
    app_data_dir: tempfile::TempDir,
    session: tokio::sync::Mutex<String>,
}

impl CliClient {
    pub async fn new(http: HttpConfig, org_id: String, user_api_key: ApiKey, master_password: String) -> Result<Self, KidsError> {
        let app_data_dir = tempfile::Builder::new()
            .prefix("kids-vaultwarden-bw-")
            .tempdir()
            .map_err(|error| KidsError::InternalError(anyhow::anyhow!(format!("Failed to create bw CLI data directory: {error}"))))?;

        let mut client = Self {
            http,
            org_id,
            user_api_key,
            master_password,
            app_data_dir,
            session: tokio::sync::Mutex::new(String::new()),
        };
        *client.session.get_mut() = client.open_session().await?;
        Ok(client)
    }

    pub async fn confirm_member(&self, member_id: &str) -> Result<(), KidsError> {
        self.run_in_session(
            "Confirming member",
            &["confirm", "org-member", member_id, "--organizationid", &self.org_id],
            None,
        )
        .await?;
        tracing::info!(member_id, "Confirmed member");
        Ok(())
    }

    pub async fn get_collections(&self) -> Result<Vec<Collection>, KidsError> {
        let output = self
            .run_in_session("Fetching collections", &["list", "org-collections", "--organizationid", &self.org_id], None)
            .await?;
        serde_json::from_str(&output).map_err(|error| KidsError::RequestFailed("Fetching collections".to_string(), anyhow!(error)))
    }

    pub async fn create_collection(&self, name: &str, external_id: &str, groups: &[Access]) -> Result<String, KidsError> {
        let output = self
            .write_collection(
                "Creating collection",
                &["create", "org-collection", "--organizationid", &self.org_id],
                name,
                external_id,
                groups,
                &[],
            )
            .await?;
        let collection: dto::CreatedCollection =
            serde_json::from_str(&output).map_err(|error| KidsError::RequestFailed("Creating collection".to_string(), anyhow!(error)))?;
        tracing::info!(name, external_id, collection_id = collection.id, "Created collection");
        Ok(collection.id)
    }

    /// Replaces the collection's name, group access and member access.
    pub async fn update_collection(&self, collection_id: &str, name: &str, external_id: &str, groups: &[Access], members: &[Access]) -> Result<(), KidsError> {
        self.write_collection(
            "Updating collection",
            &["edit", "org-collection", collection_id, "--organizationid", &self.org_id],
            name,
            external_id,
            groups,
            members,
        )
        .await?;
        tracing::info!(collection_id, name, "Updated collection");
        Ok(())
    }

    /// Runs a `create` or `edit` command with the collection encoded as `bw` expects it on stdin.
    async fn write_collection(
        &self,
        context: &str,
        args: &[&str],
        name: &str,
        external_id: &str,
        groups: &[Access],
        users: &[Access],
    ) -> Result<String, KidsError> {
        let template = dto::CollectionTemplate {
            organization_id: &self.org_id,
            name,
            external_id,
            groups,
            users,
        };
        let json =
            serde_json::to_string(&template).map_err(|error| KidsError::InternalError(anyhow::anyhow!(format!("Failed to serialize collection: {error}"))))?;
        let encoded = self.run("Encoding collection for bw CLI", &["encode"], &[], Some(&json)).await?;
        self.run_in_session(context, args, Some(&encoded)).await
    }

    /// Logs in and unlocks the vault, returning the session key.
    async fn open_session(&self) -> Result<String, KidsError> {
        // Fails when nobody is logged in, which is fine.
        let _ = self.spawn(&["logout"], &[], None).await;
        self.run("Configuring bw CLI server", &["config", "server", self.http.url.as_str()], &[], None)
            .await?;
        let credentials = [
            ("BW_CLIENTID", self.user_api_key.client_id.as_str()),
            ("BW_CLIENTSECRET", self.user_api_key.client_secret.as_str()),
        ];
        self.run("Logging in to bw CLI", &["login", "--apikey"], &credentials, None).await?;
        let session = self
            .run(
                "Unlocking bw CLI vault",
                &["unlock", "--raw", "--passwordenv", "BW_PASSWORD"],
                &[("BW_PASSWORD", &self.master_password)],
                None,
            )
            .await?;
        tracing::info!("Logged in to bw CLI and unlocked vault");
        Ok(session)
    }

    /// Runs a command that needs an unlocked vault, opening a new session once if the current one is no longer valid.
    async fn run_in_session(&self, context: &str, args: &[&str], stdin: Option<&str>) -> Result<String, KidsError> {
        let mut session = self.session.lock().await;
        let mut output = self.spawn(args, &[("BW_SESSION", &session)], stdin).await?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() && INVALID_SESSION_MESSAGES.iter().any(|message| stderr.contains(message)) {
            tracing::info!("bw CLI session is no longer valid, opening a new one");
            *session = self.open_session().await?;
            output = self.spawn(args, &[("BW_SESSION", &session)], stdin).await?;
        }
        into_stdout(context, args, output)
    }

    /// Runs a command, failing if it exits unsuccessfully, and returns its trimmed stdout.
    async fn run(&self, context: &str, args: &[&str], env: &[(&str, &str)], stdin: Option<&str>) -> Result<String, KidsError> {
        let output = self.spawn(args, env, stdin).await?;
        into_stdout(context, args, output)
    }

    async fn spawn(&self, args: &[&str], env: &[(&str, &str)], stdin: Option<&str>) -> Result<std::process::Output, KidsError> {
        use tokio::io::AsyncWriteExt;
        let io_error = |error: std::io::Error| KidsError::RequestFailed(format!("Running `bw {}`", args.join(" ")), anyhow!(error));

        let mut command = tokio::process::Command::new("bw");
        command
            .arg("--nointeraction")
            .args(args)
            .env("BITWARDENCLI_APPDATA_DIR", self.app_data_dir.path())
            .envs(env.iter().copied())
            .stdin(if stdin.is_some() {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            })
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if self.http.insecure_disable_tls_verification {
            // `bw` is a Node application.
            command.env("NODE_TLS_REJECT_UNAUTHORIZED", "0");
        }

        let mut child = command.spawn().map_err(io_error)?;
        if let Some(input) = stdin {
            let mut child_stdin = child.stdin.take().expect("child was spawned with a piped stdin");
            child_stdin.write_all(input.as_bytes()).await.map_err(io_error)?;
        }
        child.wait_with_output().await.map_err(io_error)
    }
}

fn into_stdout(context: &str, args: &[&str], output: std::process::Output) -> Result<String, KidsError> {
    if !output.status.success() {
        return Err(KidsError::RequestFailed(
            context.to_string(),
            anyhow!(
                "`bw {}` exited with {}: {}",
                args.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
