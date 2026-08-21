//! Thin HTTP client for the Hetzner Cloud API.
//!
//! Tools pass endpoint paths and JSON through; this module owns auth, the
//! base URL, and mapping the API's `{"error": {...}}` envelope into errors.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

const BASE_URL: &str = "https://api.hetzner.cloud/v1";
const MAX_ERROR_BODY_CHARS: usize = 500;
/// Hard cap on a decoded response body; Hetzner responses are small JSON
/// envelopes, so anything past this is a misbehaving peer, not real data.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Authenticated client for one Hetzner Cloud project.
#[derive(Clone)]
pub struct HcloudClient {
    http: reqwest::Client,
    token: String,
    base_url: String,
}

impl HcloudClient {
    /// Build a client for `token` against `HCLOUD_ENDPOINT` (optional base
    /// URL override, same convention as the official hcloud CLI) or the real
    /// API. The caller resolves `HCLOUD_TOKEN` (single- or multi-project).
    pub fn from_env(token: impl Into<String>) -> Result<Self> {
        Self::new(endpoint(std::env::var("HCLOUD_ENDPOINT").ok())?, token)
    }

    /// Clone this client with a different project's token, sharing the same
    /// underlying `reqwest::Client` (and its connection pool) - swapping
    /// projects per call must not rebuild the HTTP layer each time.
    pub fn with_token(&self, token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            ..self.clone()
        }
    }

    /// Build a client against any base URL (tests point this at wiremock).
    /// Fails when the TLS backend cannot load a platform trust store.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context(
                "failed to build the HTTP client - is a platform TLS trust store available?",
            )?;
        Ok(Self {
            http,
            token: token.into(),
            base_url: base_url.into(),
        })
    }

    pub async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let req = self.http.get(self.url(path)?).query(query);
        self.send(req, path).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let req = self.http.post(self.url(path)?).json(&body);
        self.send(req, path).await
    }

    pub async fn put(&self, path: &str, body: Value) -> Result<Value> {
        let req = self.http.put(self.url(path)?).json(&body);
        self.send(req, path).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        let req = self.http.delete(self.url(path)?);
        self.send(req, path).await
    }

    /// Ids are typed u64 at the tool layer; this guards the shared layer
    /// anyway - reqwest's Url normalizes "..", which would escape /v1.
    /// '%' is rejected too: a caller-controlled percent-escape could decode
    /// into any of the other rejected characters after the URL is built.
    fn url(&self, path: &str) -> Result<String> {
        if !path.starts_with('/')
            || path.contains("..")
            || path.contains('?')
            || path.contains('#')
            || path.contains('%')
        {
            bail!("invalid API path: {path}");
        }
        Ok(format!("{}{path}", self.base_url))
    }

    async fn send(&self, req: reqwest::RequestBuilder, path: &str) -> Result<Value> {
        let mut resp = req
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
        // Stream chunks rather than buffering via `.text()`, so a body past
        // the cap is caught before it's fully read into memory.
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .with_context(|| format!("reading the response body from {path} failed"))?
        {
            if bytes.len() + chunk.len() > MAX_BODY_BYTES {
                bail!("response body from {path} exceeds the {MAX_BODY_BYTES}-byte limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = String::from_utf8(bytes)
            .with_context(|| format!("the response from {path} is not valid UTF-8"))?;
        if !status.is_success() {
            let hint = retry_after
                .map(|s| format!(" (Retry-After: {s})"))
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

/// The effective base URL: a non-empty `HCLOUD_ENDPOINT` wins (after
/// rejecting a non-https, non-loopback override), else the real API.
fn endpoint(env_value: Option<String>) -> Result<String> {
    let Some(v) = env_value
        .map(|v| v.trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
    else {
        return Ok(BASE_URL.to_string());
    };
    if !v.starts_with("https://") && !is_loopback_http(&v) {
        bail!(
            "HCLOUD_ENDPOINT must use https:// (or http:// to 127.0.0.1/::1/localhost), got: {v}"
        );
    }
    Ok(v)
}

/// Whether `v` is an `http://` URL whose host is a loopback address - the
/// only case a plaintext `HCLOUD_ENDPOINT` override is allowed (wiremock in
/// tests; `HcloudClient::new` itself stays scheme-agnostic for the same reason).
fn is_loopback_http(v: &str) -> bool {
    let Some(rest) = v.strip_prefix("http://") else {
        return false;
    };
    // A bracketed IPv6 literal (e.g. "[::1]:1/v1") contains ':' itself, so it
    // must be extracted before splitting the host from an optional port.
    let host = if let Some(inner) = rest.strip_prefix('[') {
        inner.split(']').next().unwrap_or("")
    } else {
        rest.split(['/', ':']).next().unwrap_or("")
    };
    host == "127.0.0.1" || host == "::1" || host == "localhost"
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
    fn endpoint_defaults_to_the_real_api_and_honours_a_non_empty_override() {
        assert_eq!(endpoint(None).unwrap(), BASE_URL);
        assert_eq!(endpoint(Some(String::new())).unwrap(), BASE_URL);
        assert_eq!(
            endpoint(Some("https://internal.example/v1".into())).unwrap(),
            "https://internal.example/v1"
        );
        assert_eq!(
            endpoint(Some("http://127.0.0.1:1/v1".into())).unwrap(),
            "http://127.0.0.1:1/v1"
        );
        assert_eq!(
            endpoint(Some("http://127.0.0.1:1/v1/".into())).unwrap(),
            "http://127.0.0.1:1/v1"
        );
        assert_eq!(
            endpoint(Some("http://localhost:1/v1".into())).unwrap(),
            "http://localhost:1/v1"
        );
        assert_eq!(
            endpoint(Some("http://[::1]:1/v1".into())).unwrap(),
            "http://[::1]:1/v1"
        );
    }

    /// W2.3: a plaintext, non-loopback override is rejected - the env is
    /// trusted, but a typo'd `http://` should not silently ship credentials
    /// in the clear to some other host.
    #[test]
    fn endpoint_rejects_a_non_https_non_loopback_override() {
        let err = endpoint(Some("http://api.hetzner.cloud/v1".into())).unwrap_err();
        assert!(err.to_string().contains("https://"), "got: {err}");
    }

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
    async fn with_token_swaps_only_the_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/locations"))
            .and(header("authorization", "Bearer other-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "locations": []
            })))
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
        let swapped = client.with_token("other-token");
        let v = swapped.get("/locations", &[]).await.unwrap();
        assert_eq!(v["locations"], serde_json::json!([]));
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

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
        let v = client.get("/locations", &[]).await.unwrap();
        assert_eq!(v["locations"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn traversal_paths_are_rejected_before_any_request() {
        // Base URL points nowhere; the guard must fire before a connection.
        let client = HcloudClient::new("http://127.0.0.1:9", "test-token").unwrap();
        for bad in [
            "/ssh_keys/../servers/42",
            "/servers?admin=1",
            "/a#frag",
            "/a%2e%2e/b",
            "servers",
        ] {
            let err = client.get(bad, &[]).await.unwrap_err();
            assert!(
                err.to_string().contains("invalid API path"),
                "path {bad} got: {err}"
            );
        }
    }

    /// W2.2: a response body past the cap is rejected instead of being
    /// buffered in full - proven with a body just one byte over the limit.
    #[tokio::test]
    async fn response_bodies_past_the_cap_are_rejected() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(vec![b'a'; MAX_BODY_BYTES + 1], "application/json"),
            )
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
        let err = client.get("/servers", &[]).await.unwrap_err();
        assert!(err.to_string().contains("byte limit"), "got: {err}");
    }

    /// W2.2: Hetzner never redirects, so the client must not silently follow
    /// one - a redirect response is surfaced as its raw (non-success) status.
    #[tokio::test]
    async fn redirects_are_not_followed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .respond_with(
                ResponseTemplate::new(307).insert_header("location", "https://evil.example/steal"),
            )
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
        let err = client.get("/servers", &[]).await.unwrap_err();
        assert!(err.to_string().contains("307"), "got: {err}");
    }

    #[tokio::test]
    async fn put_sends_bearer_auth_and_the_json_body() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/servers/1"))
            .and(header("authorization", "Bearer test-token"))
            .and(wiremock::matchers::body_json(
                serde_json::json!({"name": "web"}),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"server": {"id": 1}})),
            )
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
        let v = client
            .put("/servers/1", serde_json::json!({"name": "web"}))
            .await
            .unwrap();
        assert_eq!(v["server"]["id"], 1);
    }

    #[tokio::test]
    async fn empty_success_body_becomes_a_success_object() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/ssh_keys/7"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
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

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
        let err = client.get("/servers", &[]).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("rate_limit_exceeded") && msg.contains("Retry-After: 3600"),
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

        let client = HcloudClient::new(server.uri(), "test-token").unwrap();
        let err = client.get("/servers/9", &[]).await.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("404") && msg.contains("not_found"),
            "got: {msg}"
        );
    }
}
