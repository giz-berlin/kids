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
    /// Must be specified if Sentry is `active`.
    pub dsn: String,
    /// Tag specifying which context the service is running in (for example, development, production, ...).
    /// Must be specified if Sentry is `active`.
    pub environment: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct ControllerConfig {
    /// Address with port to bind the HTTP server to.
    pub bind_addr: String,
    /// Interval in seconds to perform the full sync from source to target.
    #[serde(default = "default_full_sync_interval")]
    pub full_sync_interval_seconds: u64,
    /// If present, serve the API over HTTPS using this certificate/key.
    /// If absent, the API is served over plain HTTP.
    #[serde(default)]
    pub tls: Option<TlsConfig>,
}

fn default_full_sync_interval() -> u64 {
    // Default to every 24h for performing a full sync from source to target.
    24 * 60 * 60
}

#[derive(serde::Deserialize, Debug)]
pub struct TlsConfig {
    /// PEM-encoded server certificate to present to clients.
    pub cert_pem: String,
    /// PEM-encoded private key belonging to `cert_pem`.
    pub key_pem: String,
    /// If present, enables mandatory mTLS: every connection must present one of the
    /// pinned client certificates below, or the TLS handshake is rejected.
    #[serde(default)]
    pub client_auth: Option<ClientAuthConfig>,
}

#[derive(serde::Deserialize, Debug)]
pub struct ClientAuthConfig {
    /// Pinned client certificates, each identifying one named client allowed to connect.
    pub clients: Vec<ClientCertConfig>,
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
            [source]

            [controller]
            bind_addr = "127.0.0.1:8080"

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        assert!(config.sentry.is_none());
    }

    #[test]
    fn test_try_from_str_tls_absent() {
        let toml_str = r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [source]

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        assert!(config.controller.tls.is_none());
    }

    #[test]
    fn test_try_from_str_tls_server_only() {
        let toml_str = r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            cert_pem = "-----BEGIN CERTIFICATE-----\nserver-cert\n-----END CERTIFICATE-----"
            key_pem = "-----BEGIN PRIVATE KEY-----\nserver-key\n-----END PRIVATE KEY-----"

            [source]

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        let tls = config.controller.tls.unwrap();
        assert!(tls.cert_pem.contains("server-cert"));
        assert!(tls.key_pem.contains("server-key"));
        assert!(tls.client_auth.is_none());
    }

    #[test]
    fn test_try_from_str_tls_client_auth() {
        let toml_str = r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            cert_pem = "-----BEGIN CERTIFICATE-----\nserver-cert\n-----END CERTIFICATE-----"
            key_pem = "-----BEGIN PRIVATE KEY-----\nserver-key\n-----END PRIVATE KEY-----"

            [controller.tls.client_auth]

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
        let clients = config.controller.tls.unwrap().client_auth.unwrap().clients;
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
