use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn requests_advertise_gzip() {
    let server = MockServer::start().await;
    // This mock ONLY matches when Accept-Encoding contains gzip;
    // otherwise wiremock 404s and the call errors.
    Mock::given(method("GET"))
        .and(path("/probe"))
        .and(header("accept-encoding", "gzip"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;
    let c = hetzner_mcp::hcloud::HcloudClient::new(server.uri(), "t");
    let v = c.get("/probe", &[]).await.expect("gzip header missing -> mock 404 -> this fails");
    assert_eq!(v["ok"], true);
}
