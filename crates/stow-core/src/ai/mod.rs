//! AI providers: a tiny [`AiClient`] trait over the Anthropic Messages API and the
//! `OpenAI` chat completions API, keychain-backed key storage, and prompt building.

pub mod anthropic;
#[cfg(feature = "keychain")]
pub mod keys;
pub mod openai;
pub mod prompts;
pub mod run;

pub use crate::models::Provider;
pub use prompts::{build_prompt, AiContext};
pub use run::{load_context, run_ai, run_ai_with};

use std::time::Duration;

use crate::{Result, StowError};

/// One chat-completion backend.
#[async_trait::async_trait]
pub trait AiClient: Send + Sync {
    /// Send one system + user turn; return the text reply.
    async fn complete(&self, system: &str, user: &str) -> Result<String>;
    /// Which provider this is.
    fn provider(&self) -> Provider;
    /// Model id sent with each request.
    fn model(&self) -> &str;
}

/// Everything needed to build an [`AiClient`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    /// Which API.
    pub provider: Provider,
    /// Model override; `None`/blank = [`default_model`].
    pub model: Option<String>,
    /// API key; `None`/blank is an error.
    pub api_key: Option<String>,
    /// Base URL override (tests, proxies).
    pub base_url: Option<String>,
}

/// Default model per provider.
pub fn default_model(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAi => "gpt-5-mini",
        Provider::Anthropic => "claude-sonnet-5-5",
    }
}

/// Build the client for `cfg`. A missing key → [`StowError::Provider`] with no status.
pub fn select_provider(cfg: ProviderConfig) -> Result<Box<dyn AiClient>> {
    let ProviderConfig {
        provider,
        model,
        api_key,
        base_url,
    } = cfg;
    let api_key = match api_key {
        Some(k) if !k.trim().is_empty() => k,
        _ => {
            return Err(StowError::Provider {
                provider: provider.as_str(),
                status: None,
                message: "no api key".into(),
            })
        }
    };
    let model = match model {
        Some(m) if !m.trim().is_empty() => m,
        _ => default_model(provider).to_string(),
    };
    Ok(match provider {
        Provider::Anthropic => {
            Box::new(anthropic::AnthropicProvider::new(api_key, model, base_url)?)
        }
        Provider::OpenAi => Box::new(openai::OpenAiProvider::new(api_key, model, base_url)?),
    })
}

/// Completions can take a minute on long inputs, but must never hang a job forever.
pub(crate) fn api_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .build()?)
}

/// First ≤300 chars of an error body, for [`StowError::Provider`] messages.
pub(crate) fn excerpt(body: &str) -> String {
    let body = body.trim();
    match body.char_indices().nth(300) {
        Some((idx, _)) => format!("{}…", &body[..idx]),
        None => body.to_string(),
    }
}

/// Reads `ai_provider` / `ai_model` from settings and the key from the keychain.
pub fn provider_from_settings(db: &crate::Db) -> Result<Box<dyn AiClient>> {
    use crate::repo::settings::{self, Key};
    let provider = settings::validate_ai_provider(&settings::get_key(db, Key::AiProvider)?)?;
    let model = settings::get(db, Key::AiModel.as_str())?.filter(|m| !m.trim().is_empty());
    let api_key = api_key_for(provider)?;
    select_provider(ProviderConfig {
        provider,
        model,
        api_key,
        base_url: None,
    })
}

#[cfg(feature = "keychain")]
fn api_key_for(provider: Provider) -> Result<Option<String>> {
    keys::get(keys::Account::from(provider))
}

#[cfg(not(feature = "keychain"))]
fn api_key_for(_provider: Provider) -> Result<Option<String>> {
    Err(StowError::Keychain(
        "keychain support is not available in this build".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: Provider, model: Option<&str>, key: Option<&str>) -> ProviderConfig {
        ProviderConfig {
            provider,
            model: model.map(str::to_string),
            api_key: key.map(str::to_string),
            base_url: None,
        }
    }

    #[test]
    fn default_model_values() {
        assert_eq!(default_model(Provider::Anthropic), "claude-sonnet-5-5");
        assert_eq!(default_model(Provider::OpenAi), "gpt-5-mini");
    }

    #[test]
    fn provider_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Provider::OpenAi).unwrap(),
            r#""openai""#
        );
        assert_eq!(
            serde_json::from_str::<Provider>(r#""anthropic""#).unwrap(),
            Provider::Anthropic
        );
        assert!("bard".parse::<Provider>().is_err());
    }

    #[test]
    fn select_provider_without_key_errors() {
        let err = select_provider(cfg(Provider::Anthropic, None, None))
            .err()
            .unwrap();
        assert!(
            matches!(err, StowError::Provider { provider: "anthropic", status: None, ref message } if message == "no api key"),
            "{err:?}"
        );
        assert_eq!(err.to_string(), "anthropic: no api key");
        let err = select_provider(cfg(Provider::OpenAi, None, Some("")))
            .err()
            .unwrap();
        assert_eq!(err.code(), "provider");
    }

    #[test]
    fn select_provider_uses_defaults_and_overrides() {
        let p = select_provider(cfg(Provider::Anthropic, None, Some("k"))).unwrap();
        assert_eq!(p.provider(), Provider::Anthropic);
        assert_eq!(p.model(), "claude-sonnet-5-5");
        let p = select_provider(cfg(Provider::OpenAi, Some("gpt-x"), Some("k"))).unwrap();
        assert_eq!(p.provider(), Provider::OpenAi);
        assert_eq!(p.model(), "gpt-x");
    }
}
