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
            .expect("failed to build the HTTP client - is a platform TLS trust store available?");
        Self {
            http,
            token: token.into(),
            base_url: base_url.into(),
        }
    }

    pub async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let req = self.http.get(self.url(path)?).query(query);
        self.send(req, path).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let req = self.http.post(self.url(path)?).json(&body);
        self.send(req, path).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        let req = self.http.delete(self.url(path)?);
        self.send(req, path).await
    }

    /// Ids are typed u64 at the tool layer; this guards the shared layer
    /// anyway - reqwest's Url normalizes "..", which would escape /v1.
    fn url(&self, path: &str) -> Result<String> {
        if path.contains("..") || path.contains('?') || path.contains('#') {
            bail!("invalid API path: {path}");
        }
        Ok(format!("{}{path}", self.base_url))
    }

    async fn send(&self, req: reqwest::RequestBuilder, path: &str) -> Result<Value> {
        let resp = req
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("request to {path} failed"))?;
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = resp
            .text()
            .await
            .with_context(|| format!("reading the response body from {path} failed"))?;
        if !status.is_success() {
            let hint = retry_after
                .map(|s| format!(" (retry after {s}s)"))
                .unwrap_or_default();
            bail!(
                "Hetzner API {path} returned {status}: {}{hint}",
                api_error_summary(&body)
            );
        }
        if body.is_empty() {
            // e.g. DELETE /ssh_keys/{id} returns 204 with no body; a bare
            // `null` result reads like a broken tool to the model.
            return Ok(serde_json::json!({"success": true}));
        }
        serde_json::from_str(&body)
            .with_context(|| format!("the response from {path} is not valid JSON"))
    }
}

/// `{"error": {"code", "message", "details"?}}` -> "code: message (details)";
/// anything else -> the raw body, truncated.
fn api_error_summary(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body)
        && let Some(e) = v.get("error")
        && e.is_object()
    {
        let code = e.get("code").and_then(Value::as_str).unwrap_or("unknown");
        let message = e.get("message").and_then(Value::as_str).unwrap_or("");
        return match e.get("details") {
            Some(d) if !d.is_null() => format!("{code}: {message} (details: {d})"),
            _ => format!("{code}: {message}"),
        };
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
    fn api_error_summary_appends_details_when_present() {
        let body = r#"{"error":{"code":"invalid_input","message":"bad field","details":{"fields":[{"name":"server_type"}]}}}"#;
        let s = api_error_summary(body);
        assert!(
            s.starts_with("invalid_input: bad field (details:") && s.contains("server_type"),
            "got: {s}"
        );
    }

    #[test]
    fn api_error_summary_falls_back_when_error_is_not_an_object() {
        let body = r#"{"error":"upstream gateway timeout"}"#;
        assert_eq!(api_error_summary(body), body);
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
    async fn traversal_paths_are_rejected_before_any_request() {
        // Base URL points nowhere; the guard must fire before a connection.
        let client = HcloudClient::new("http://127.0.0.1:9", "test-token");
        for bad in ["/ssh_keys/../servers/42", "/servers?admin=1", "/a#frag"] {
            let err = client.get(bad, &[]).await.unwrap_err();
            assert!(
                err.to_string().contains("invalid API path"),
                "path {bad} got: {err}"
            );
        }
    }

    #[tokio::test]
    async fn empty_success_body_becomes_a_success_object() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/ssh_keys/7"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token");
        let v = client.delete("/ssh_keys/7").await.unwrap();
        assert_eq!(v, serde_json::json!({"success": true}));
    }

    #[tokio::test]
    async fn rate_limit_responses_carry_the_retry_hint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "3600")
                    .set_body_json(serde_json::json!({
                        "error": {"code": "rate_limit_exceeded", "message": "limit reached"}
                    })),
            )
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token");
        let err = client.get("/servers", &[]).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("rate_limit_exceeded") && msg.contains("retry after 3600s"),
            "got: {msg}"
        );
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
