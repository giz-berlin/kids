use anyhow::Context;

#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config<S, T> {
    pub sentry: Option<SentryConfig>,
    pub http: HTTPConfig,
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
pub struct HTTPConfig {
    /// Address with port to bind the HTTP server to.
    pub bind_addr: String,
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

            [target]
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        assert!(config.sentry.is_none());
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
