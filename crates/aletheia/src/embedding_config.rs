//! Runtime translation for `[embedding]` config.

use std::fmt;

use koina::secret::SecretString;
use koina::system::{Environment, RealSystem};
use mneme::embedding::EmbeddingConfig;
use taxis::config::EmbeddingSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderSupport {
    Available,
    MissingFeature(&'static str),
}

/// Runtime embedding configuration error with operator-actionable categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EmbeddingConfigIssue {
    EmptyProvider,
    UnsupportedProvider {
        provider: String,
    },
    MissingFeature {
        provider: String,
        feature: &'static str,
    },
    MissingEndpoint {
        provider: String,
        field: &'static str,
    },
    MissingCredential {
        provider: String,
        env_var: String,
    },
}

impl fmt::Display for EmbeddingConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProvider => write!(f, "embedding.provider must not be empty"),
            Self::UnsupportedProvider { provider } => write!(
                f,
                "unsupported embedding provider '{provider}'; choose candle, openai-compat, or voyage (mock is test-only)"
            ),
            Self::MissingFeature { provider, feature } => write!(
                f,
                "embedding.provider = \"{provider}\" is unavailable in this build; rebuild aletheia with --features {feature}"
            ),
            Self::MissingEndpoint { provider, field } => {
                write!(f, "embedding.provider = \"{provider}\" requires {field}")
            }
            Self::MissingCredential { provider, env_var } => write!(
                f,
                "embedding.provider = \"{provider}\" requires a credential in environment variable {env_var}; set embedding.apiKeyEnv or {env_var} before starting aletheia"
            ),
        }
    }
}

impl std::error::Error for EmbeddingConfigIssue {}

fn provider_support(provider: &str) -> Option<ProviderSupport> {
    match provider {
        "mock" if cfg!(test) => Some(ProviderSupport::Available),
        "mock" => Some(ProviderSupport::MissingFeature("test-support")),
        "candle" if cfg!(feature = "embed-candle") => Some(ProviderSupport::Available),
        "candle" => Some(ProviderSupport::MissingFeature("embed-candle")),
        "openai-compat" | "voyage" if cfg!(feature = "openai-embed") => {
            Some(ProviderSupport::Available)
        }
        "openai-compat" | "voyage" => Some(ProviderSupport::MissingFeature("openai-embed")),
        _ => None,
    }
}

/// Validate `[embedding]` against this binary's enabled provider features and
/// required runtime environment.
pub(crate) fn validate_embedding_settings(
    settings: &EmbeddingSettings,
) -> Result<(), EmbeddingConfigIssue> {
    runtime_embedding_config(settings).map(|_| ())
}

/// Convert `[embedding]` into the provider config, resolving configured
/// credential environment variables without exposing the secret value.
pub(crate) fn runtime_embedding_config(
    settings: &EmbeddingSettings,
) -> Result<EmbeddingConfig, EmbeddingConfigIssue> {
    runtime_embedding_config_with(settings, provider_support, |name| RealSystem.var(name))
}

fn runtime_embedding_config_with(
    settings: &EmbeddingSettings,
    provider_support: impl Fn(&str) -> Option<ProviderSupport>,
    read_env: impl Fn(&str) -> Option<String>,
) -> Result<EmbeddingConfig, EmbeddingConfigIssue> {
    let provider = settings.provider.trim();
    if provider.is_empty() {
        return Err(EmbeddingConfigIssue::EmptyProvider);
    }

    match provider_support(provider) {
        Some(ProviderSupport::Available) => {}
        Some(ProviderSupport::MissingFeature(feature)) => {
            return Err(EmbeddingConfigIssue::MissingFeature {
                provider: provider.to_owned(),
                feature,
            });
        }
        None => {
            return Err(EmbeddingConfigIssue::UnsupportedProvider {
                provider: provider.to_owned(),
            });
        }
    }

    if settings
        .base_url
        .as_deref()
        .is_some_and(|base_url| base_url.trim().is_empty())
    {
        return Err(EmbeddingConfigIssue::MissingEndpoint {
            provider: provider.to_owned(),
            field: "embedding.baseUrl",
        });
    }

    if provider == "openai-compat" && settings.base_url.is_none() {
        return Err(EmbeddingConfigIssue::MissingEndpoint {
            provider: provider.to_owned(),
            field: "embedding.baseUrl",
        });
    }

    let api_key = resolve_api_key(provider, settings, read_env)?;
    let mut config = settings.to_embedding_config_with_api_key(api_key);
    provider.clone_into(&mut config.provider);
    config.base_url = settings
        .base_url
        .as_deref()
        .map(str::trim)
        .map(str::to_owned);
    Ok(config)
}

fn resolve_api_key(
    provider: &str,
    settings: &EmbeddingSettings,
    read_env: impl Fn(&str) -> Option<String>,
) -> Result<Option<SecretString>, EmbeddingConfigIssue> {
    let configured_env = settings.api_key_env.as_deref().map(str::trim);
    let Some(env_var) = configured_env.filter(|env_var| !env_var.is_empty()) else {
        return match provider {
            "voyage" => read_required_api_key(provider, "VOYAGE_API_KEY", read_env),
            _ => Ok(None),
        };
    };

    read_required_api_key(provider, env_var, read_env)
}

fn read_required_api_key(
    provider: &str,
    env_var: &str,
    read_env: impl Fn(&str) -> Option<String>,
) -> Result<Option<SecretString>, EmbeddingConfigIssue> {
    read_env(env_var)
        .filter(|value| !value.is_empty())
        .map(SecretString::from)
        .map(Some)
        .ok_or_else(|| EmbeddingConfigIssue::MissingCredential {
            provider: provider.to_owned(),
            env_var: env_var.to_owned(),
        })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn settings(provider: &str) -> EmbeddingSettings {
        EmbeddingSettings {
            provider: provider.to_owned(),
            dimension: 384,
            ..EmbeddingSettings::default()
        }
    }

    fn available_provider(provider: &str) -> Option<ProviderSupport> {
        (!provider.is_empty()).then_some(ProviderSupport::Available)
    }

    #[cfg(feature = "embed-candle")]
    #[test]
    fn default_candle_embedding_config_is_available() {
        let config = runtime_embedding_config(&EmbeddingSettings::default())
            .expect("default candle embedding config should be available");
        assert_eq!(config.provider, "candle");
        assert_eq!(config.dimension, Some(384));
    }

    #[cfg(not(feature = "openai-embed"))]
    #[test]
    fn openai_compat_reports_missing_feature_in_default_build() {
        let mut settings = settings("openai-compat");
        settings.base_url = Some("http://127.0.0.1:5005/v1".to_owned());

        let err = runtime_embedding_config(&settings)
            .expect_err("openai-compat should require openai-embed in default build");
        assert!(matches!(
            err,
            EmbeddingConfigIssue::MissingFeature {
                provider,
                feature: "openai-embed",
            } if provider == "openai-compat"
        ));
    }

    #[test]
    fn unsupported_provider_is_rejected() {
        let err = runtime_embedding_config(&settings("telepathy"))
            .expect_err("unknown provider should fail validation");
        assert!(matches!(
            err,
            EmbeddingConfigIssue::UnsupportedProvider { provider } if provider == "telepathy"
        ));
    }

    #[test]
    fn openai_compat_requires_base_url_when_provider_is_available() {
        let err =
            runtime_embedding_config_with(&settings("openai-compat"), available_provider, |_| None)
                .expect_err("openai-compat should require baseUrl");
        assert!(matches!(
            err,
            EmbeddingConfigIssue::MissingEndpoint {
                provider,
                field: "embedding.baseUrl",
            } if provider == "openai-compat"
        ));
    }

    #[test]
    fn voyage_requires_credential_when_provider_is_available() {
        let err = runtime_embedding_config_with(&settings("voyage"), available_provider, |_| None)
            .expect_err("voyage should require an API key");
        assert!(matches!(
            err,
            EmbeddingConfigIssue::MissingCredential {
                provider,
                env_var,
            } if provider == "voyage" && env_var == "VOYAGE_API_KEY"
        ));
    }

    #[test]
    fn configured_api_key_env_is_resolved_without_requiring_default_env_name() {
        let mut settings = settings("voyage");
        settings.api_key_env = Some("ALETHEIA_TEST_VOYAGE_KEY".to_owned());

        let config = runtime_embedding_config_with(&settings, available_provider, |name| {
            (name == "ALETHEIA_TEST_VOYAGE_KEY").then(|| "secret-value".to_owned())
        })
        .expect("configured apiKeyEnv should resolve");
        assert!(config.api_key.is_some());
    }
}
