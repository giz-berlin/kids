use anyhow::Context;

#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config<S, T> {
    pub sentry: Option<SentryConfig>,
    pub source: Option<S>,
    pub target: Option<T>,
}

#[derive(serde::Deserialize, Debug)]
pub struct EmptyConfig {}

#[derive(serde::Deserialize, Debug)]
pub struct SentryConfig {
    pub active: bool,
    pub dsn: String,
    pub environment: String,
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
            active = true
            dsn = "https://example@sentry.io/123"
            environment = "production"
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        assert!(config.sentry.is_some());
        let sentry = config.sentry.unwrap();
        assert_eq!(sentry.dsn, "https://example@sentry.io/123");
        assert_eq!(sentry.environment, "production");
    }

    #[test]
    fn test_try_from_str_no_sentry() {
        let toml_str = r#"
            # No sentry section
        "#;
        let config = Config::<EmptyConfig, EmptyConfig>::try_from_str(toml_str).unwrap();
        assert!(config.sentry.is_none());
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
