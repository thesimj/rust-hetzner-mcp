//! Shared test helpers used by the per-domain tool test modules.

use rmcp::model::CallToolResult;

use crate::hcloud::HcloudClient;

use super::HcloudServer;

/// Build a server whose client talks to the given mock Hetzner base URL.
pub(crate) fn server_for(uri: String) -> HcloudServer {
    HcloudServer::new(HcloudClient::new(uri, "test-token"))
}

/// Extract the JSON the tool wrote into its text content block.
pub(crate) fn tool_result_json(res: &CallToolResult) -> serde_json::Value {
    let v = serde_json::to_value(res).unwrap();
    let text = v["content"][0]["text"].as_str().expect("text content");
    serde_json::from_str(text).unwrap()
}
