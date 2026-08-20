//! The rmcp stdio MCP server. Tool implementations are split by domain into
//! submodules; each contributes a `#[tool_router]`-generated router that
//! [`HcloudServer::new`] combines into the router the handler dispatches through.

use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{
        CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities,
        ServerInfo,
    },
    tool_handler,
    transport::stdio,
};
use serde_json::Value;

use crate::hcloud::HcloudClient;

mod compute;
mod infra;

#[cfg(test)]
mod test_support;

/// MCP server wrapping an [`HcloudClient`].
#[derive(Clone)]
pub struct HcloudServer {
    pub(crate) client: HcloudClient,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl HcloudServer {
    pub fn new(client: HcloudClient) -> Self {
        Self {
            client,
            tool_router: Self::compute_router() + Self::infra_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for HcloudServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "MCP server for the Hetzner Cloud API. Read tools (list_servers, \
                get_server, list_images, list_server_types, list_locations, \
                list_datacenters, list_volumes, list_networks, list_firewalls, \
                list_ssh_keys and their get_* variants) are safe to call freely. \
                create_server creates a billable resource and create_ssh_key \
                adds a persistent credential; delete_server, power_server, and \
                delete_ssh_key are destructive - confirm with the user before \
                calling any of these.",
            );
        // The objective pins the latest MCP revision; rmcp does not default to
        // it, so name it explicitly (pinned by a test below).
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        // rmcp's default Implementation reports the SDK as the server; name ourselves.
        info.server_info = Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        info
    }
}

/// Serialize a tool's JSON payload into the MCP text content block.
/// Compact on purpose: the consumer is a model, and pretty-printing nearly
/// doubles the token cost of every payload.
pub fn ok_json(value: Value) -> Result<CallToolResult, ErrorData> {
    let json = serde_json::to_string(&value)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
}

/// Map an upstream API failure into a tool-level error result (isError), so
/// the model sees the message and can recover. Protocol errors stay reserved
/// for dispatch failures.
pub fn map_api_err(e: anyhow::Error) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!("{e:#}"))])
}

/// Turn an upstream call into the tool's result: success passes through
/// `ok_json`; failure becomes an `isError` `CallToolResult` (`map_api_err`),
/// never a protocol-level error, so the model sees it and can recover.
pub(crate) fn respond(result: anyhow::Result<Value>) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(value) => ok_json(value),
        Err(e) => Ok(map_api_err(e)),
    }
}

/// Standard `page`/`per_page` query pair, omitting unset values.
pub(crate) fn pagination_query(
    page: Option<u32>,
    per_page: Option<u32>,
) -> Vec<(&'static str, String)> {
    let mut query = Vec::new();
    if let Some(page) = page {
        query.push(("page", page.to_string()));
    }
    if let Some(per_page) = per_page {
        query.push(("per_page", per_page.to_string()));
    }
    query
}

/// Push an optional string query param, skipping `None` AND empty strings -
/// Hetzner rejects empty values (e.g. `?label_selector=`) with 400.
pub(crate) fn push_param(
    query: &mut Vec<(&'static str, String)>,
    key: &'static str,
    value: Option<String>,
) {
    if let Some(v) = value
        && !v.is_empty()
    {
        query.push((key, v));
    }
}

/// Start the stdio MCP server and run until the client disconnects.
pub async fn run() -> anyhow::Result<()> {
    let client = HcloudClient::from_env()?;
    let service = HcloudServer::new(client).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the advertised handshake to the latest MCP revision (objective
    /// requirement). An rmcp upgrade must not silently move this.
    #[test]
    fn advertises_latest_protocol_and_tools_only() {
        let info = test_support::server_for("http://127.0.0.1:9".to_string()).get_info();
        assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
        assert_eq!(info.server_info.name, "hetzner-mcp");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.capabilities.tools.is_some(), "tools are advertised");
        assert!(info.capabilities.prompts.is_none());
        assert!(info.capabilities.resources.is_none());
        assert!(info.instructions.is_some());
    }

    #[test]
    fn ok_json_wraps_the_payload_in_a_text_block() {
        let res = ok_json(serde_json::json!({"servers": []})).unwrap();
        assert_eq!(
            test_support::tool_result_json(&res),
            serde_json::json!({"servers": []})
        );
    }

    /// `+` on ToolRouter silently overwrites on name collision; pin the count.
    #[test]
    fn the_combined_router_registers_all_23_tools() {
        let server = test_support::server_for("http://127.0.0.1:9".to_string());
        assert_eq!(server.tool_router.list_all().len(), 23);
    }

    #[test]
    fn push_param_skips_none_and_empty_values() {
        let mut q = pagination_query(Some(2), None);
        push_param(&mut q, "label_selector", Some(String::new()));
        push_param(&mut q, "name", None);
        push_param(&mut q, "status", Some("running".into()));
        assert_eq!(q, vec![("page", "2".into()), ("status", "running".into())]);
    }

    #[test]
    fn map_api_err_returns_a_tool_error_result_with_the_message() {
        let res = map_api_err(anyhow::anyhow!("boom"));
        assert_eq!(res.is_error, Some(true));
        let v = serde_json::to_value(&res).unwrap();
        let text = v["content"][0]["text"].as_str().expect("text content");
        assert!(text.contains("boom"), "got: {text}");
    }
}
