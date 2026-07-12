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
    pub tls: Tls,
}

fn default_full_sync_interval() -> u64 {
    // Default to every 24h for performing a full sync from source to target.
    24 * 60 * 60
}

#[derive(serde::Deserialize, Debug)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Tls {
    /// Serve plain HTTP. Do not use in production.
    InsecureDisabled,
    Enabled {
        /// PEM-encoded server certificate chain to present to clients.
        #[serde(rename = "cert_pem")]
        cert: ServerCertChain,
        /// PEM-encoded private key belonging to `cert`.
        #[serde(rename = "key_pem")]
        key: ServerPrivateKey,
        /// Client authentication configuration.
        client_auth: ClientAuth,
    },
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
    #[serde(rename = "cert_pem")]
    pub cert: ClientCert,
    /// Whether this client may additionally reach the webhook routes (/v1/users, /v1/groups).
    /// If false, the client can only reach the health and docs routes.
    #[serde(default)]
    pub allow_webhook_access: bool,
}

/// Parsed server certificate chain.
pub struct ServerCertChain(pub Vec<rustls::pki_types::CertificateDer<'static>>);

impl std::fmt::Debug for ServerCertChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ServerCertChain({} cert(s))", self.0.len())
    }
}

impl<'de> serde::Deserialize<'de> for ServerCertChain {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pem = String::deserialize(deserializer)?;
        let certs = rustls_pemfile::certs(&mut pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?;
        if certs.is_empty() {
            return Err(serde::de::Error::custom("no certificates found in PEM input"));
        }
        Ok(ServerCertChain(certs))
    }
}

/// Parsed server private key.
pub struct ServerPrivateKey(pub rustls::pki_types::PrivateKeyDer<'static>);

impl std::fmt::Debug for ServerPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[redacted]")
    }
}

impl<'de> serde::Deserialize<'de> for ServerPrivateKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pem = String::deserialize(deserializer)?;
        let key = rustls_pemfile::private_key(&mut pem.as_bytes())
            .map_err(serde::de::Error::custom)?
            .ok_or_else(|| serde::de::Error::custom("no private key found in PEM input"))?;
        Ok(ServerPrivateKey(key))
    }
}

/// Single parsed client certificate.
pub struct ClientCert(pub rustls::pki_types::CertificateDer<'static>);

impl std::fmt::Debug for ClientCert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClientCert({} bytes)", self.0.as_ref().len())
    }
}

impl<'de> serde::Deserialize<'de> for ClientCert {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pem = String::deserialize(deserializer)?;
        let mut certs = rustls_pemfile::certs(&mut pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)?;
        match certs.len() {
            0 => Err(serde::de::Error::custom("no certificate found in PEM input")),
            1 => Ok(ClientCert(certs.remove(0))),
            n => Err(serde::de::Error::custom(format!("expected exactly one certificate, found {n}"))),
        }
    }
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

    fn generate_cert_and_key() -> (String, String) {
        let rcgen::CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(vec!["test".to_string()]).unwrap();
        (cert.pem(), signing_key.serialize_pem())
    }

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
        let (cert_pem, key_pem) = generate_cert_and_key();
        let toml_str = format!(
            r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            mode = "enabled"
            cert_pem = """{cert_pem}"""
            key_pem = """{key_pem}"""

            [controller.tls.client_auth]
            mode = "insecure_disabled"

            [source]

            [target]
            "#
        );
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(&toml_str).unwrap();
        let Tls::Enabled { cert, client_auth, .. } = config.controller.tls else {
            panic!("expected Tls::Enabled");
        };
        assert_eq!(cert.0.len(), 1);
        assert!(matches!(client_auth, ClientAuth::InsecureDisabled));
    }

    #[test]
    fn test_try_from_str_tls_enabled_client_auth_enabled() {
        let (server_cert_pem, server_key_pem) = generate_cert_and_key();
        let (keycloak_cert_pem, _) = generate_cert_and_key();
        let (monitoring_cert_pem, _) = generate_cert_and_key();
        let toml_str = format!(
            r#"
            [controller]
            bind_addr = "127.0.0.1:8080"

            [controller.tls]
            mode = "enabled"
            cert_pem = """{server_cert_pem}"""
            key_pem = """{server_key_pem}"""

            [controller.tls.client_auth]
            mode = "enabled"

            [[controller.tls.client_auth.clients]]
            name = "keycloak"
            cert_pem = """{keycloak_cert_pem}"""
            allow_webhook_access = true

            [[controller.tls.client_auth.clients]]
            name = "monitoring"
            cert_pem = """{monitoring_cert_pem}"""

            [source]

            [target]
            "#
        );
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(&toml_str).unwrap();
        let Tls::Enabled { client_auth, .. } = config.controller.tls else {
            panic!("expected Tls::Enabled");
        };
        let ClientAuth::Enabled { clients } = client_auth else {
            panic!("expected ClientAuth::Enabled");
        };
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0].name, "keycloak");
        assert!(clients[0].allow_webhook_access);
        assert!(!clients[0].cert.0.is_empty());
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

    #[test]
    fn test_try_from_str_invalid_pem() {
        let (valid_cert_pem, valid_key_pem) = generate_cert_and_key();
        let (valid_client_cert_pem, _) = generate_cert_and_key();

        let cases = [
            (
                "invalid server cert_pem",
                "not a certificate",
                valid_key_pem.as_str(),
                valid_client_cert_pem.as_str(),
            ),
            (
                "invalid server key_pem",
                valid_cert_pem.as_str(),
                "not a private key",
                valid_client_cert_pem.as_str(),
            ),
            ("invalid client cert_pem", valid_cert_pem.as_str(), valid_key_pem.as_str(), "not a certificate"),
        ];

        for (description, cert_pem, key_pem, client_cert_pem) in cases {
            let toml_str = format!(
                r#"
                [controller]
                bind_addr = "127.0.0.1:8080"

                [controller.tls]
                mode = "enabled"
                cert_pem = """{cert_pem}"""
                key_pem = """{key_pem}"""

                [controller.tls.client_auth]
                mode = "enabled"

                [[controller.tls.client_auth.clients]]
                name = "test-client"
                cert_pem = """{client_cert_pem}"""

                [source]
                [target]
                "#
            );
            assert!(
                Config::<EmptyConfig, EmptyConfig>::try_from_str(&toml_str).is_err(),
                "expected error for: {description}",
            );
        }
    }
}
