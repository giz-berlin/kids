use std::collections::HashMap;

use clap::{Args, Parser, Subcommand, ValueHint};
use reqwest::Client;
use url::Url;

#[derive(Parser)]
#[command(name = "webhook-cli", about = "CLI for interacting with webhook endpoints")]
struct Cli {
    #[arg(long, value_hint = ValueHint::Url, default_value = "http://127.0.0.1:3000")]
    endpoint: Url,

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let client = Client::new();

    let resp: Option<reqwest::Response>;

    match cli.command {
        Command::User(user_cmd) => match user_cmd {
            UserCommand::List => {
                let url = cli.endpoint.join("/v1/users")?;
                resp = Some(client.get(url).send().await?);
            }
            UserCommand::Upsert(args) => {
                let user_id = args.id;
                let user = kids::source::keycloak::KeycloakWebhookUser {
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
                let group = kids::source::keycloak::KeycloakWebhookGroup {
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
