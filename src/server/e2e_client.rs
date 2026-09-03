//! W2.4: in-process end-to-end test. Drives the real MCP handshake
//! (initialize -> tools/list -> tools/call) through an actual client/server
//! pair connected by an in-memory duplex pipe, exercising `call_tool`
//! itself rather than calling a tool method directly. Needs rmcp's "client"
//! feature, added as a dev-dependency only (never ships in the binary).

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, Implementation};

use super::test_support::project_token;
use super::{HcloudServer, Projects};
use crate::config::Project;
use crate::hcloud::HcloudClient;

fn project(name: &str) -> Project {
    Project {
        name: name.to_string(),
        token: project_token(name),
        description: None,
    }
}

/// Startup is lazy: building the server and completing the real MCP
/// handshake (initialize + tools/list) must send zero HTTP requests. The
/// Hetzner API is first contacted by an actual tools/call, never before.
#[tokio::test]
async fn startup_and_handshake_send_no_http_requests() {
    let mock = wiremock::MockServer::start().await;

    let projects = Projects::new(vec![project("prod")], None);
    let client = HcloudClient::new(mock.uri(), project_token("prod")).unwrap();
    let server = HcloudServer::new(client, projects);

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server handshake");
        running.waiting().await.expect("server run loop");
    });

    let client = ().serve(client_io).await.expect("client handshake");
    client.list_tools(None).await.expect("tools/list");

    let requests = mock.received_requests().await.unwrap_or_default();
    assert!(
        requests.is_empty(),
        "startup/handshake must not contact the Hetzner API, saw: {requests:?}"
    );

    client.cancel().await.expect("cancel client");
    server_task.await.expect("server task");
}

#[tokio::test]
async fn initialize_list_and_call_flow_through_the_real_call_tool() {
    let mock = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/servers"))
        .and(wiremock::matchers::header(
            "authorization",
            format!("Bearer {}", project_token("staging")),
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"servers": []})),
        )
        .mount(&mock)
        .await;
    // list_projects fans a fingerprint probe out to every project's token.
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/servers"))
        .and(wiremock::matchers::header(
            "authorization",
            format!("Bearer {}", project_token("prod")),
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"servers": []})),
        )
        .mount(&mock)
        .await;

    let projects = Projects::new(vec![project("prod"), project("staging")], None);
    let client = HcloudClient::new(mock.uri(), project_token("prod")).unwrap();
    let server = HcloudServer::new(client, projects);

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server_task = tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server handshake");
        running.waiting().await.expect("server run loop");
    });

    let client = ().serve(client_io).await.expect("client handshake");

    // initialize: the real server identity is visible after the handshake.
    let peer_info = client
        .peer_info()
        .expect("peer info retained after initialize");
    assert_eq!(
        peer_info.server_info,
        Some(Implementation::new(
            "hetzner-mcp",
            env!("CARGO_PKG_VERSION")
        ))
    );

    // tools/list: the real combined router, not a direct method call.
    let tools = client.list_tools(None).await.expect("tools/list");
    assert_eq!(tools.tools.len(), 93);
    assert!(tools.tools.iter().any(|t| t.name == "list_servers"));

    // tools/call: the real call_tool dispatch - project resolution, token
    // swap, and multi-project result annotation, all through the wire.
    let mut args = serde_json::Map::new();
    args.insert("project".to_string(), serde_json::json!("staging"));
    let result = client
        .call_tool(CallToolRequestParams::new("list_servers").with_arguments(args))
        .await
        .expect("tools/call");
    assert_ne!(result.is_error, Some(true));
    let text = result.content[0].as_text().unwrap().text.clone();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({"project": "staging", "result": {"servers": []}})
    );

    // D3 item 5: list_projects must stay reachable with n>1 and no `project`
    // argument at all - it is project-independent (regression class: B3).
    let list_projects_result = client
        .call_tool(CallToolRequestParams::new("list_projects"))
        .await
        .expect("tools/call list_projects");
    assert_ne!(list_projects_result.is_error, Some(true));

    client.cancel().await.expect("cancel client");
    server_task.await.expect("server task");
}
