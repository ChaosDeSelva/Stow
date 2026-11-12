//! `OpenAI` chat completions API (`POST {base}/v1/chat/completions`).

use serde_json::{json, Value};

use super::{AiClient, Provider};
use crate::{Result, StowError};

/// Production endpoint.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com";

/// Client for the `OpenAi` API.
pub struct OpenAiProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
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
impl AiClient for OpenAiProvider {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });
        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(StowError::Provider {
                provider: "openai",
                status: Some(status.as_u16()),
                message: super::excerpt(&text),
            });
        }
        let v: Value = serde_json::from_str(&text)?;
        match v["choices"][0]["message"]["content"].as_str() {
            Some(c) if !c.is_empty() => Ok(c.to_string()),
            _ => Err(StowError::Provider {
                provider: "openai",
                status: Some(status.as_u16()),
                message: "no message content in the response".into(),
            }),
        }
    }

    fn provider(&self) -> Provider {
        Provider::OpenAi
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
    async fn openai_provider_parses_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-oa"))
            .and(header("content-type", "application/json"))
            .and(body_partial_json(json!({
                "model": "gpt-5-mini",
                "messages": [
                    {"role": "system", "content": "SYS"},
                    {"role": "user", "content": "USER"}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-1",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hi there"},
                    "finish_reason": "stop"
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let p =
            OpenAiProvider::new("sk-oa".into(), "gpt-5-mini".into(), Some(server.uri())).unwrap();
        let out = p.complete("SYS", "USER").await.unwrap();
        assert_eq!(out, "Hi there");
        assert_eq!(p.provider(), Provider::OpenAi);
        assert_eq!(p.model(), "gpt-5-mini");
    }

    #[tokio::test]
    async fn openai_error_status_is_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_raw(r#"{"error":{"message":"bad key"}}"#, "application/json"),
            )
            .mount(&server)
            .await;
        let p = OpenAiProvider::new("k".into(), "gpt-5-mini".into(), Some(server.uri())).unwrap();
        let err = p.complete("s", "u").await.err().unwrap();
        assert!(
            matches!(
                err,
                StowError::Provider {
                    status: Some(401),
                    ..
                }
            ),
            "{err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.starts_with("openai 401") && msg.contains("bad key"),
            "{msg}"
        );
    }

    #[tokio::test]
    async fn openai_missing_content_is_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"choices": []})))
            .mount(&server)
            .await;
        let p = OpenAiProvider::new("k".into(), "gpt-5-mini".into(), Some(server.uri())).unwrap();
        let err = p.complete("s", "u").await.err().unwrap();
        assert!(
            matches!(
                err,
                StowError::Provider {
                    provider: "openai",
                    ..
                }
            ),
            "{err:?}"
        );
    }
}
