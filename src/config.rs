use anyhow::Context;

#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config<S, T> {
    pub sentry: Option<SentryConfig>,
    pub controller: ControllerConfig,
    pub source: S,
    pub target: T,
}

#[derive(serde::Deserialize, Debug)]
pub struct EmptyConfig {}

#[derive(serde::Deserialize, Debug)]
pub struct SentryConfig {
    /// Sentry Data Source Name (DSN). Tells Sentry where to send events to so they're associated with the correct project.
    pub dsn: String,
    /// Tag specifying which context the service is running in (for example, development, production, ...).
    pub environment: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct ControllerConfig {
    /// Address with port to bind the HTTP server to.
    pub bind_addr: std::net::SocketAddr,
    /// Interval in seconds to perform the full sync from source to target.
    #[serde(default = "default_full_sync_interval")]
    pub full_sync_interval_seconds: u64,
    /// TLS configuration.
    pub tls: Tls,
}

fn default_full_sync_interval() -> u64 {
    // Default to every 24h for performing a full sync from source to target.
    24 * 60 * 60
}

#[derive(serde::Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Tls {
    /// Serve plain HTTP. Do not use in production.
    InsecureDisabled,
    Enabled {
        /// PEM-encoded server certificate to present to clients.
        cert_pem: String,
        /// PEM-encoded private key belonging to `cert_pem`.
        key_pem: String,
        /// Client authentication configuration.
        client_auth: ClientAuth,
    },
}

// Debug implementation for the TLS struct to prevent the key_pem from leaking into logs.
impl std::fmt::Debug for Tls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tls::InsecureDisabled => {
                f.write_str("InsecureDisabled")
            }
            Tls::Enabled {
                cert_pem,
                key_pem: _,
                client_auth,
            } => {
                f.debug_struct("Enabled")
                    .field("cert_pem", cert_pem)
                    .field("key_pem", &"[REDACTED]")
                    .field("client_auth", client_auth)
                    .finish()
            }
        }
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ClientAuth {
    /// Disable client certificate authentication. Do not use in production.
    InsecureDisabled,
    /// Every connection must present one of the pinned client certificates
    /// or the TLS handshake is rejected.
    Enabled { clients: Vec<ClientCertConfig> },
}

#[derive(serde::Deserialize, Debug)]
pub struct ClientCertConfig {
    /// Identifies this client in logs/tracing.
    pub name: String,
    /// PEM-encoded client certificate to pin.
    pub cert_pem: String,
    /// Whether this client may additionally reach the webhook routes (/v1/users, /v1/groups).
    /// If false, the client can only reach the health and docs routes.
    #[serde(default)]
    pub allow_webhook_access: bool,
}

impl<S: serde::de::DeserializeOwned, T: serde::de::DeserializeOwned> Config<S, T> {
    pub fn try_from_str(content: &str) -> anyhow::Result<Self> {
        toml::from_str(content).context("Failed to parse config content")
    }
}

impl<S: serde::de::DeserializeOwned, T: serde::de::DeserializeOwned> TryFrom<&std::path::Path> for Config<S, T> {
    type Error = anyhow::Error;

    fn try_from(path: &std::path::Path) -> Result<Self, Self::Error> {
        let content = std::fs::read_to_string(path).with_context(|| format!("Failed to read config file: {}", path.display()))?;

        Self::try_from_str(&content).with_context(|| format!("Failed to parse config file: {}", path.display()))
    }
}

impl<S: serde::de::DeserializeOwned, T: serde::de::DeserializeOwned> TryFrom<std::path::PathBuf> for Config<S, T> {
    type Error = anyhow::Error;

    fn try_from(path: std::path::PathBuf) -> Result<Self, Self::Error> {
        Self::try_from(path.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_from_str_valid() {
        let toml_str = r#"
            [sentry]
            dsn = "https://example@sentry.io/123"
            environment = "production"

            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            mode = "insecure_disabled"

            [source]

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        let sentry = config.sentry;
        assert!(sentry.is_some());
        assert_eq!(sentry.as_ref().unwrap().dsn, "https://example@sentry.io/123");
        assert_eq!(sentry.as_ref().unwrap().environment, "production");
    }

    #[test]
    fn test_try_from_str_sentry_inactive() {
        let toml_str = r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            mode = "insecure_disabled"

            [source]

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        assert!(config.sentry.is_none());
    }

    #[test]
    fn test_try_from_str_tls_insecure_disabled() {
        let toml_str = r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            mode = "insecure_disabled"

            [source]

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        assert!(matches!(config.controller.tls, Tls::InsecureDisabled));
    }

    #[test]
    fn test_try_from_str_tls_enabled_client_auth_disabled() {
        let toml_str = r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            mode = "enabled"
            cert_pem = "-----BEGIN CERTIFICATE-----\nserver-cert\n-----END CERTIFICATE-----"
            key_pem = "-----BEGIN PRIVATE KEY-----\nserver-key\n-----END PRIVATE KEY-----"

            [controller.tls.client_auth]
            mode = "insecure_disabled"

            [source]

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        let Tls::Enabled {
            cert_pem,
            key_pem,
            client_auth,
        } = config.controller.tls
        else {
            panic!("expected Tls::Enabled");
        };
        assert!(cert_pem.contains("server-cert"));
        assert!(key_pem.contains("server-key"));
        assert!(matches!(client_auth, ClientAuth::InsecureDisabled));
    }

    #[test]
    fn test_try_from_str_tls_enabled_client_auth_enabled() {
        let toml_str = r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            mode = "enabled"
            cert_pem = "-----BEGIN CERTIFICATE-----\nserver-cert\n-----END CERTIFICATE-----"
            key_pem = "-----BEGIN PRIVATE KEY-----\nserver-key\n-----END PRIVATE KEY-----"

            [controller.tls.client_auth]
            mode = "enabled"

            [[controller.tls.client_auth.clients]]
            name = "keycloak"
            cert_pem = "-----BEGIN CERTIFICATE-----\nkeycloak-cert\n-----END CERTIFICATE-----"
            allow_webhook_access = true

            [[controller.tls.client_auth.clients]]
            name = "monitoring"
            cert_pem = "-----BEGIN CERTIFICATE-----\nmonitoring-cert\n-----END CERTIFICATE-----"

            [source]

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        let Tls::Enabled { client_auth, .. } = config.controller.tls else {
            panic!("expected Tls::Enabled");
        };
        let ClientAuth::Enabled { clients } = client_auth else {
            panic!("expected ClientAuth::Enabled");
        };
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0].name, "keycloak");
        assert!(clients[0].allow_webhook_access);
        assert!(clients[0].cert_pem.contains("keycloak-cert"));
        assert_eq!(clients[1].name, "monitoring");
        assert!(!clients[1].allow_webhook_access);
    }

    #[test]
    fn test_try_from_str_missing_section() {
        let toml_str = r#"
            [sentry]
            active = false
        "#;
        let result = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_try_from_str_invalid_toml() {
        let toml_str = r#"
            [sentry
            dsn = "https://example@sentry.io/123"
        "#;
        let result = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str);
        assert!(result.is_err());
    }
}
