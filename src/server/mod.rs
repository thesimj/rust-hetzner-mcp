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
mod lb_zone_ops;
mod misc;
mod net;
mod netres_ops;
mod res_ops;
mod servers_ops;

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
            tool_router: Self::compute_router()
                + Self::infra_router()
                + Self::net_router()
                + Self::misc_router()
                + Self::servers_ops_router()
                + Self::res_ops_router()
                + Self::netres_ops_router()
                + Self::lb_zone_ops_router(),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for HcloudServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "MCP server for the Hetzner Cloud API. Every list_*/get_* tool \
                is read-only and safe to call freely. Tools named create_*, \
                update_*, delete_*, power_server, and *_action mutate real \
                resources: creates may bill money, deletes are permanent, \
                actions can interrupt workloads - confirm with the user before \
                calling any of them.",
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
pub(crate) fn ok_json(value: Value) -> Result<CallToolResult, ErrorData> {
    let json = serde_json::to_string(&value)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
}

/// Map an upstream API failure into a tool-level error result (isError), so
/// the model sees the message and can recover. Protocol errors stay reserved
/// for dispatch failures.
pub(crate) fn map_api_err(e: anyhow::Error) -> CallToolResult {
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

/// Reject an update body with no fields set: `skip_serializing_if` makes an
/// all-unset call serialize to `{}`, which would send a mutating no-op PUT.
/// Every update_* tool calls this between `to_value` and `client.put`.
pub(crate) fn require_update_fields(body: &Value) -> Result<(), ErrorData> {
    if body.as_object().is_none_or(serde_json::Map::is_empty) {
        Err(ErrorData::invalid_params(
            "set at least one field to update",
            None,
        ))
    } else {
        Ok(())
    }
}

/// Numeric ID of a single resource, shared by the by-id tools across seams.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct IdArgs {
    /// Numeric ID of the resource, from the matching list_* tool's response
    /// (or, for actions, a mutation response's `action.id`).
    pub id: u64,
}

/// ID or name of a zone - the only string this crate interpolates into a URL path.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct ZoneIdArgs {
    /// Zone ID or name, from list_zones. ASCII letters, digits, '.', and '-'
    /// only; not "." or containing "..".
    pub id_or_name: String,
}

/// Maximum length of a DNS zone name (RFC 1035).
pub(crate) const MAX_ZONE_ID_LEN: usize = 253;

/// Reject anything but a bare, non-dot-segment path component before it
/// reaches the URL: "." collapses to the collection endpoint (wire-confirmed)
/// and ".." can walk the path elsewhere.
pub(crate) fn validate_zone_id(id_or_name: &str) -> Result<(), ErrorData> {
    let valid = !id_or_name.is_empty()
        && id_or_name.len() <= MAX_ZONE_ID_LEN
        && id_or_name != "."
        && !id_or_name.contains("..")
        && id_or_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if valid {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "id_or_name must be non-empty, at most {MAX_ZONE_ID_LEN} characters, must not \
                 be \".\" or contain \"..\", and must contain only ASCII letters, digits, '.', \
                 or '-'"
            ),
            None,
        ))
    }
}

/// Turn an action's optional params object into a POST body - `{}` when unset.
pub(crate) fn action_body(params: Option<serde_json::Map<String, Value>>) -> Value {
    params.map_or_else(|| serde_json::json!({}), Value::Object)
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

    /// `+` on ToolRouter silently overwrites on name collision; the combined
    /// count must equal the sum of the parts at any count.
    #[test]
    fn the_combined_router_loses_no_tool_to_a_name_collision() {
        let server = test_support::server_for("http://127.0.0.1:9".to_string());
        let parts = HcloudServer::compute_router().list_all().len()
            + HcloudServer::infra_router().list_all().len()
            + HcloudServer::net_router().list_all().len()
            + HcloudServer::misc_router().list_all().len()
            + HcloudServer::servers_ops_router().list_all().len()
            + HcloudServer::res_ops_router().list_all().len()
            + HcloudServer::netres_ops_router().list_all().len()
            + HcloudServer::lb_zone_ops_router().list_all().len();
        assert_eq!(server.tool_router.list_all().len(), parts);
        // Absolute pin: 13 compute + 10 infra + 8 net + 8 misc + 6 servers_ops
        // + 15 res_ops + 16 netres_ops + 16 lb_zone_ops.
        assert_eq!(server.tool_router.list_all().len(), 92);
    }

    #[test]
    fn push_param_skips_none_and_empty_values() {
        let mut q = pagination_query(Some(2), None);
        push_param(&mut q, "label_selector", Some(String::new()));
        push_param(&mut q, "name", None);
        push_param(&mut q, "status", Some("running".into()));
        assert_eq!(q, vec![("page", "2".into()), ("status", "running".into())]);
    }

    /// Crate-wide: every tool must carry all three hints and a distinct,
    /// non-empty title, whatever its router pins locally.
    #[test]
    fn every_tool_carries_full_annotations_and_a_distinct_title() {
        let server = test_support::server_for("http://127.0.0.1:9".to_string());
        let mut titles = std::collections::HashSet::new();
        for tool in server.tool_router.list_all() {
            let a = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{}: missing annotations", tool.name));
            assert!(a.read_only_hint.is_some(), "{}: read_only_hint", tool.name);
            assert!(
                a.destructive_hint.is_some(),
                "{}: destructive_hint",
                tool.name
            );
            assert_eq!(
                a.open_world_hint,
                Some(true),
                "{}: open_world_hint",
                tool.name
            );
            let title = a.title.clone().unwrap_or_default();
            assert!(!title.is_empty(), "{}: empty title", tool.name);
            assert!(titles.insert(title), "{}: duplicate title", tool.name);
        }
        assert_eq!(titles.len(), 92);
    }

    #[test]
    fn map_api_err_returns_a_tool_error_result_with_the_message() {
        let res = map_api_err(anyhow::anyhow!("boom"));
        assert_eq!(res.is_error, Some(true));
        let v = serde_json::to_value(&res).unwrap();
        let text = v["content"][0]["text"].as_str().expect("text content");
        assert!(text.contains("boom"), "got: {text}");
    }

    #[test]
    fn require_update_fields_rejects_only_an_empty_object() {
        assert!(require_update_fields(&serde_json::json!({})).is_err());
        assert!(require_update_fields(&serde_json::json!({"name": "x"})).is_ok());
    }

    /// N1: every one of the 13 update_* tools must reject an all-unset call
    /// with a protocol error, before any HTTP request is attempted (dead
    /// port - a skipped guard would surface as an isError, not this Err).
    #[tokio::test]
    async fn every_update_tool_rejects_an_all_unset_call_without_making_a_request() {
        use rmcp::handler::server::wrapper::Parameters;
        use rmcp::model::ErrorCode;

        use super::lb_zone_ops::{UpdateLoadBalancerArgs, UpdateZoneArgs, UpdateZoneRrsetArgs};
        use super::netres_ops::{
            UpdateFirewallArgs, UpdateFloatingIpArgs, UpdateNetworkArgs, UpdatePrimaryIpArgs,
        };
        use super::res_ops::{UpdateImageArgs, UpdateNameLabelsArgs};
        use super::servers_ops::UpdateServerArgs;

        let server = test_support::server_for("http://127.0.0.1:9".to_string());

        macro_rules! assert_rejects {
            ($call:expr) => {
                assert_eq!($call.await.unwrap_err().code, ErrorCode::INVALID_PARAMS);
            };
        }

        assert_rejects!(server.update_image(Parameters(UpdateImageArgs {
            id: 1,
            description: None,
            r#type: None,
            labels: None
        })));
        assert_rejects!(server.update_ssh_key(Parameters(UpdateNameLabelsArgs {
            id: 1,
            name: None,
            labels: None
        })));
        assert_rejects!(server.update_volume(Parameters(UpdateNameLabelsArgs {
            id: 1,
            name: None,
            labels: None
        })));
        assert_rejects!(
            server.update_placement_group(Parameters(UpdateNameLabelsArgs {
                id: 1,
                name: None,
                labels: None
            }))
        );
        assert_rejects!(server.update_certificate(Parameters(UpdateNameLabelsArgs {
            id: 1,
            name: None,
            labels: None
        })));
        assert_rejects!(server.update_network(Parameters(UpdateNetworkArgs {
            id: 1,
            name: None,
            labels: None,
            expose_routes_to_vswitch: None
        })));
        assert_rejects!(server.update_firewall(Parameters(UpdateFirewallArgs {
            id: 1,
            name: None,
            labels: None
        })));
        assert_rejects!(server.update_floating_ip(Parameters(UpdateFloatingIpArgs {
            id: 1,
            description: None,
            labels: None,
            name: None
        })));
        assert_rejects!(server.update_primary_ip(Parameters(UpdatePrimaryIpArgs {
            id: 1,
            name: None,
            auto_delete: None,
            labels: None
        })));
        assert_rejects!(
            server.update_load_balancer(Parameters(UpdateLoadBalancerArgs {
                id: 1,
                name: None,
                labels: None
            }))
        );
        assert_rejects!(server.update_zone(Parameters(UpdateZoneArgs {
            id_or_name: "example.com".into(),
            labels: None
        })));
        assert_rejects!(server.update_zone_rrset(Parameters(UpdateZoneRrsetArgs {
            id_or_name: "example.com".into(),
            rr_name: "www".into(),
            rr_type: "A".into(),
            labels: None
        })));
        assert_rejects!(server.update_server(Parameters(UpdateServerArgs {
            id: 1,
            name: None,
            labels: None
        })));
    }
}
