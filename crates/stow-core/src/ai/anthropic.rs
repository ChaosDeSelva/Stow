//! Anthropic Messages API (`POST {base}/v1/messages`).

use serde_json::{json, Value};

use super::{AiClient, Provider};
use crate::{Result, StowError};

/// Production endpoint.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// Per the claude-api skill (curl/examples.md): `anthropic-version: 2026-10-21`.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
const MAX_TOKENS: u32 = 2048;

/// Client for the Anthropic API.
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    /// `base_url` defaults to [`DEFAULT_BASE_URL`]. Fails only if TLS setup fails.
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Result<Self> {
        let base_url = base_url
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        Ok(Self {
            api_key,
            model,
            base_url,
            client: super::api_client()?,
        })
    }
}

#[async_trait::async_trait]
impl AiClient for AnthropicProvider {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "max_tokens": MAX_TOKENS,
            "system": system,
            "messages": [{"role": "user", "content": user}],
        });
        let resp = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(StowError::Provider {
                provider: "anthropic",
                status: Some(status.as_u16()),
                message: super::excerpt(&text),
            });
        }
        let v: Value = serde_json::from_str(&text)?;
        let out: String = v["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        if out.is_empty() {
            return Err(StowError::Provider {
                provider: "anthropic",
                status: Some(status.as_u16()),
                message: format!(
                    "no text in the response (stop_reason: {})",
                    v["stop_reason"].as_str().unwrap_or("unknown")
                ),
            });
        }
        Ok(out)
    }

    fn provider(&self) -> Provider {
        Provider::Anthropic
    }

    fn model(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn anthropic_provider_parses_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "sk-test"))
            .and(header("anthropic-version", "2026-10-21"))
            .and(header("content-type", "application/json"))
            .and(body_partial_json(json!({
                "model": "claude-sonnet-5-5",
                "max_tokens": 2048,
                "system": "SYS",
                "messages": [{"role": "user", "content": "USER"}]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": ""},
                    {"type": "text", "text": "Hello "},
                    {"type": "text", "text": "world"}
                ],
                "stop_reason": "end_turn"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let p = AnthropicProvider::new(
            "sk-test".into(),
            "claude-sonnet-5-5".into(),
            Some(server.uri()),
        )
        .unwrap();
        let out = p.complete("SYS", "USER").await.unwrap();
        assert_eq!(out, "Hello world");
        assert_eq!(p.provider(), Provider::Anthropic);
        assert_eq!(p.model(), "claude-sonnet-5-5");
    }

    #[tokio::test]
    async fn provider_error_status_is_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(500).set_body_raw(
                r#"{"error":{"type":"api_error","message":"boom"}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;

        let p = AnthropicProvider::new(
            "sk-test".into(),
            "claude-sonnet-5-5".into(),
            Some(server.uri()),
        )
        .unwrap();
        let err = p.complete("s", "u").await.err().unwrap();
        match err {
            StowError::Provider {
                provider: "anthropic",
                status: Some(500),
                ref message,
            } => {
                let msg = err.to_string();
                assert!(msg.starts_with("anthropic 500"), "{msg}");
                assert!(message.contains("boom"), "{message}");
                assert!(msg.contains("anthropic"), "{msg}");
                assert!(msg.contains("boom"), "{msg}");
            }
            other => panic!("expected Ai error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn anthropic_error_body_is_truncated() {
        let server = MockServer::start().await;
        let big = "x".repeat(5000);
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_raw(big, "text/plain"))
            .mount(&server)
            .await;
        let p = AnthropicProvider::new("k".into(), "m".into(), Some(server.uri())).unwrap();
        let err = p.complete("s", "u").await.err().unwrap();
        let StowError::Provider { message, .. } = err else {
            panic!("{err:?}")
        };
        assert!(message.len() < 400, "{}", message.len());
    }
}
