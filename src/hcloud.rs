//! Thin HTTP client for the Hetzner Cloud API.
//!
//! Tools pass endpoint paths and JSON through; this module owns auth, the
//! base URL, and mapping the API's `{"error": {...}}` envelope into errors.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

const BASE_URL: &str = "https://api.hetzner.cloud/v1";
const MAX_ERROR_BODY_CHARS: usize = 500;

/// Authenticated client for one Hetzner Cloud project.
#[derive(Clone)]
pub struct HcloudClient {
    http: reqwest::Client,
    token: String,
    base_url: String,
}

impl HcloudClient {
    /// Read `HCLOUD_TOKEN` and point at the real API.
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("HCLOUD_TOKEN")
            .context("HCLOUD_TOKEN environment variable is required")?;
        Ok(Self::new(BASE_URL, token))
    }

    /// Build a client against any base URL (tests point this at wiremock).
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client construction cannot fail with these options");
        Self {
            http,
            token: token.into(),
            base_url: base_url.into(),
        }
    }

    pub async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let req = self
            .http
            .get(format!("{}{path}", self.base_url))
            .query(query);
        self.send(req, path).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let req = self
            .http
            .post(format!("{}{path}", self.base_url))
            .json(&body);
        self.send(req, path).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        let req = self.http.delete(format!("{}{path}", self.base_url));
        self.send(req, path).await
    }

    async fn send(&self, req: reqwest::RequestBuilder, path: &str) -> Result<Value> {
        let resp = req
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("request to {path} failed"))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .with_context(|| format!("reading the response body from {path} failed"))?;
        if !status.is_success() {
            bail!(
                "Hetzner API {path} returned {status}: {}",
                api_error_summary(&body)
            );
        }
        if body.is_empty() {
            // e.g. DELETE /ssh_keys/{id} returns 204 with no body.
            return Ok(Value::Null);
        }
        serde_json::from_str(&body)
            .with_context(|| format!("the response from {path} is not valid JSON"))
    }
}

/// `{"error": {"code", "message"}}` -> "code: message"; anything else -> the
/// raw body, truncated.
fn api_error_summary(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body)
        && let Some(e) = v.get("error")
    {
        let code = e.get("code").and_then(Value::as_str).unwrap_or("unknown");
        let message = e.get("message").and_then(Value::as_str).unwrap_or("");
        return format!("{code}: {message}");
    }
    let mut s: String = body.chars().take(MAX_ERROR_BODY_CHARS).collect();
    if body.chars().count() > MAX_ERROR_BODY_CHARS {
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn api_error_summary_parses_the_error_envelope() {
        let body = r#"{"error":{"code":"not_found","message":"server not found"}}"#;
        assert_eq!(api_error_summary(body), "not_found: server not found");
    }

    #[test]
    fn api_error_summary_truncates_junk_bodies() {
        let body = "x".repeat(600);
        let s = api_error_summary(&body);
        assert!(s.chars().count() == MAX_ERROR_BODY_CHARS + 1 && s.ends_with('…'));
    }

    #[tokio::test]
    async fn get_sends_bearer_auth_and_parses_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/locations"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "locations": []
            })))
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token");
        let v = client.get("/locations", &[]).await.unwrap();
        assert_eq!(v["locations"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn non_success_maps_the_error_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers/9"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": "not_found", "message": "server with ID '9' not found"}
            })))
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token");
        let err = client.get("/servers/9", &[]).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("404") && msg.contains("not_found"),
            "got: {msg}"
        );
    }
}
