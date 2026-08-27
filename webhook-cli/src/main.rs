use std::collections::HashMap;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueHint};
use reqwest::Client;
use url::Url;

#[derive(Parser)]
#[command(name = "webhook-cli", about = "CLI for interacting with webhook endpoints", version)]
struct Cli {
    #[arg(long, value_hint = ValueHint::Url, default_value = "http://127.0.0.1:4165")]
    endpoint: Url,

    /// Path to a PEM certificate to trust as the server's CA, for endpoints using a self-signed
    /// server certificate not already in the system trust store.
    #[arg(long, value_hint = ValueHint::FilePath)]
    server_ca: Option<PathBuf>,

    /// Skip server certificate verification entirely. Only use for local testing.
    #[arg(long, default_value_t = false)]
    insecure: bool,

    /// Path to a PEM client certificate, for endpoints requiring mutual TLS. Requires --client-key.
    #[arg(long, value_hint = ValueHint::FilePath, requires = "client_key")]
    client_cert: Option<PathBuf>,

    /// Path to the PEM private key belonging to --client-cert.
    #[arg(long, value_hint = ValueHint::FilePath, requires = "client_cert")]
    client_key: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(subcommand)]
    User(UserCommand),
    #[command(subcommand)]
    Group(GroupCommand),
}

#[derive(Subcommand)]
enum UserCommand {
    List,
    Upsert(UserUpsertArgs),
    Delete(DeleteArgs),
}

#[derive(Subcommand)]
enum GroupCommand {
    List,
    Upsert(GroupUpsertArgs),
    Delete(DeleteArgs),
}

#[derive(Args)]
struct UserUpsertArgs {
    id: String,

    #[arg(long, default_value_t = false)]
    disabled: bool,

    #[arg(long, required = true)]
    name: String,

    #[arg(long, required = true)]
    email: String,

    /// Attributes in key=value format (can be repeated for multiple values per key)
    #[arg(long = "attribute", value_parser = parse_key_val, value_name = "KEY=VALUE")]
    attributes: Vec<(String, String)>,
}

#[derive(Args)]
struct GroupUpsertArgs {
    id: String,

    #[arg(long, required = true)]
    name: String,

    #[arg(long)]
    parent_id: Option<String>,

    #[arg(long, required = true)]
    path: String,

    /// Attributes in key=value format (can be repeated for multiple key-value pairs)
    #[arg(long = "attribute", value_parser = parse_key_val, value_name = "KEY=VALUE")]
    attributes: Vec<(String, String)>,
}

#[derive(Args)]
struct DeleteArgs {
    /// The ID to delete (user_id or group_id depending on context)
    id: String,
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err("Must be in key=value format".to_string());
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

fn attributes_to_hashmap(attrs: Vec<(String, String)>) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (key, value) in attrs {
        map.entry(key).or_default().push(value);
    }
    map
}

fn build_client(cli: &Cli) -> Result<Client, Box<dyn std::error::Error>> {
    let mut builder = Client::builder();

    if cli.insecure {
        builder = builder.danger_accept_invalid_certs(true);
    }

    if let Some(server_ca) = &cli.server_ca {
        let ca_pem = std::fs::read(server_ca).map_err(|e| format!("failed to read --server-ca {}: {e}", server_ca.display()))?;
        builder = builder.add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?);
    }

    if let Some(client_cert) = &cli.client_cert {
        // `requires = "client_key"` on the arg definition guarantees this is set.
        let client_key = cli.client_key.as_ref().unwrap();
        let cert_pem = std::fs::read(client_cert).map_err(|e| format!("failed to read --client-cert {}: {e}", client_cert.display()))?;
        let key_pem = std::fs::read(client_key).map_err(|e| format!("failed to read --client-key {}: {e}", client_key.display()))?;
        builder = builder.identity(reqwest::Identity::from_pkcs8_pem(&cert_pem, &key_pem)?);
    }

    Ok(builder.build()?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = build_client(&cli)?;

    let resp: Option<reqwest::Response>;

    match cli.command {
        Command::User(user_cmd) => match user_cmd {
            UserCommand::List => {
                let url = cli.endpoint.join("/v1/users")?;
                resp = Some(client.get(url).send().await?);
            }
            UserCommand::Upsert(args) => {
                let user_id = args.id;
                let user = source_keycloak_lib::KeycloakWebhookUser {
                    id: user_id.clone(),
                    enabled: !args.disabled,
                    username: Some(args.name),
                    email: Some(args.email),
                    attributes: attributes_to_hashmap(args.attributes),
                };

                let url = cli.endpoint.join(&format!("/v1/users/{}", user_id))?;
                resp = Some(client.put(url).json(&user).send().await?);
            }
            UserCommand::Delete(args) => {
                let url = cli.endpoint.join(&format!("/v1/users/{}", args.id))?;
                resp = Some(client.delete(url).send().await?);
            }
        },
        Command::Group(group_cmd) => match group_cmd {
            GroupCommand::List => {
                let url = cli.endpoint.join("/v1/groups")?;
                resp = Some(client.get(url).send().await?);
            }
            GroupCommand::Upsert(args) => {
                let group_id = args.id;
                let group = source_keycloak_lib::KeycloakWebhookGroup {
                    id: group_id.clone(),
                    name: args.name,
                    parent_id: args.parent_id,
                    path: args.path,
                    attributes: attributes_to_hashmap(args.attributes),
                };

                let url = cli.endpoint.join(&format!("/v1/groups/{}", group_id))?;
                resp = Some(client.put(url).json(&group).send().await?);
            }
            GroupCommand::Delete(args) => {
                let url = cli.endpoint.join(&format!("/v1/groups/{}", args.id))?;
                resp = Some(client.delete(url).send().await?);
            }
        },
    }

    if let Some(resp) = resp {
        println!("Status: {}", resp.status());
        if let Ok(body) = resp.text().await {
            println!("Body: {}", body);
        }
    }

    Ok(())
}
