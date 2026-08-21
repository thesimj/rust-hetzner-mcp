//! Shared test helpers used by the per-domain tool test modules.

use std::collections::BTreeMap;

use rmcp::model::CallToolResult;

use crate::hcloud::HcloudClient;

use super::{HcloudServer, Projects};

/// A base URL nothing listens on. Any tool that skips its own validation and
/// attempts a request has that transport failure turned into `Ok(isError)`
/// by `respond()`, so `.unwrap_err()` against a server built on this URL only
/// succeeds when validation short-circuited before the request was built.
const DEAD_PORT: &str = "http://127.0.0.1:9";

/// Single-project server pointed at [`DEAD_PORT`] (C6 - previously
/// duplicated per test module).
pub(crate) fn dead_server() -> HcloudServer {
    server_for(DEAD_PORT.to_string())
}

/// Multi-project server pointed at [`DEAD_PORT`] (C6 - previously
/// duplicated per test module).
pub(crate) fn dead_projects(names: &[&str], pin: Option<&str>) -> HcloudServer {
    server_for_projects(DEAD_PORT.to_string(), names, pin)
}

/// Build a single-project server whose client talks to the given mock
/// Hetzner base URL - the pre-multi-project shape every existing test uses.
pub(crate) fn server_for(uri: String) -> HcloudServer {
    let projects = Projects {
        tokens: BTreeMap::from([("default".to_string(), "test-token".to_string())]),
        pin: None,
    };
    HcloudServer::new(
        HcloudClient::new(uri, "test-token").expect("test client"),
        projects,
    )
}

/// Deterministic, valid-length (64-char) token for a project name, so a
/// wiremock `Bearer` assertion can tell configured projects apart.
pub(crate) fn project_token(name: &str) -> String {
    format!("{:*<64}", format!("{name}-token"))
}

/// Build a multi-project server against one mock base URL, one project per
/// name, each with its own [`project_token`]. `pin` mirrors `HCLOUD_PROJECT`.
pub(crate) fn server_for_projects(uri: String, names: &[&str], pin: Option<&str>) -> HcloudServer {
    let tokens: BTreeMap<String, String> = names
        .iter()
        .map(|n| (n.to_string(), project_token(n)))
        .collect();
    let seed = tokens
        .values()
        .next()
        .expect("at least one project")
        .clone();
    let projects = Projects {
        tokens,
        pin: pin.map(str::to_string),
    };
    HcloudServer::new(HcloudClient::new(uri, seed).expect("test client"), projects)
}

/// Extract the JSON the tool wrote into its text content block.
pub(crate) fn tool_result_json(res: &CallToolResult) -> serde_json::Value {
    let v = serde_json::to_value(res).unwrap();
    let text = v["content"][0]["text"].as_str().expect("text content");
    serde_json::from_str(text).unwrap()
}
