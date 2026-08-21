//! The rmcp stdio MCP server. Tool implementations are split by domain into
//! submodules; each contributes a `#[tool_router]`-generated router that
//! [`HcloudServer::new`] combines into the router the handler dispatches through.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
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

/// Name -> API token for every configured Hetzner project, plus the
/// operator-set `HCLOUD_PROJECT` pin (§3-§5 of the multi-project spec).
pub(crate) struct Projects {
    tokens: BTreeMap<String, String>,
    pin: Option<String>,
}

/// Names only - never derive this, a derived `Debug` would print every token.
impl std::fmt::Debug for Projects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Projects")
            .field("names", &self.tokens.keys().collect::<Vec<_>>())
            .field("pin", &self.pin)
            .finish()
    }
}

impl Projects {
    fn names_list(&self) -> String {
        self.tokens.keys().cloned().collect::<Vec<_>>().join(", ")
    }

    pub(crate) fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Resolve a call's `project` argument to a (name, token) pair, or a
    /// `-32602 invalid_params` per §4/§5. `read_only` gates whether the
    /// `HCLOUD_PROJECT` pin may stand in for a missing selector (§4 hardening,
    /// decision D2): mutating tools always require an explicit selector.
    pub(crate) fn resolve(
        &self,
        selector: Option<String>,
        read_only: bool,
    ) -> Result<(String, String), ErrorData> {
        if let [(only_name, only_token)] = &self.tokens.iter().collect::<Vec<_>>()[..] {
            return match selector {
                None => Ok(((*only_name).clone(), (*only_token).clone())),
                Some(s) if s.is_empty() || &s == *only_name => {
                    Ok(((*only_name).clone(), (*only_token).clone()))
                }
                Some(s) => Err(ErrorData::invalid_params(
                    format!("unknown project \"{s}\"; this server has one project: {only_name}"),
                    None,
                )),
            };
        }
        match selector {
            Some(s) => self
                .tokens
                .get(&s)
                .map(|t| (s.clone(), t.clone()))
                .ok_or_else(|| {
                    ErrorData::invalid_params(
                        format!(
                            "unknown project \"{s}\"; configured projects: {}",
                            self.names_list()
                        ),
                        None,
                    )
                }),
            None => self
                .pin
                .as_ref()
                .filter(|_| read_only)
                .and_then(|pin| self.tokens.get(pin).map(|t| (pin.clone(), t.clone())))
                .ok_or_else(|| {
                    ErrorData::invalid_params(
                        format!(
                            "project is required because several projects are configured; \
                             pass one of: {}",
                            self.names_list()
                        ),
                        None,
                    )
                }),
        }
    }
}

/// Parse `HCLOUD_TOKEN` per spec §3.2: a bare 64-char token names the single
/// project "default" (Form A), or comma-separated `name=token` pairs name N
/// projects (Form B). Never echoes a token value in any error (rule 6).
pub(crate) fn parse_token_env(raw: &str, pin: Option<String>) -> Result<Projects> {
    const TOKEN_LEN: usize = 64;
    let raw = raw.trim();
    let mut tokens = BTreeMap::new();
    if !raw.contains('=') {
        if raw.len() != TOKEN_LEN {
            bail!("HCLOUD_TOKEN: token must be exactly {TOKEN_LEN} characters");
        }
        tokens.insert("default".to_string(), raw.to_string());
    } else {
        let mut seen_tokens = std::collections::HashSet::new();
        for (i, entry) in raw.split(',').enumerate() {
            let idx = i + 1;
            let entry = entry.trim();
            let (name, token) = entry
                .split_once('=')
                .with_context(|| format!("HCLOUD_TOKEN entry {idx}: missing \"name=\" prefix"))?;
            let (name, token) = (name.trim(), token.trim());
            if name.is_empty() || token.is_empty() {
                bail!("HCLOUD_TOKEN entry {idx}: name and token must both be non-empty");
            }
            let name_valid = name.len() <= 64
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c));
            if !name_valid {
                bail!("HCLOUD_TOKEN entry {idx}: name must match [a-z0-9._-]{{1,64}}");
            }
            if token.len() != TOKEN_LEN {
                bail!("HCLOUD_TOKEN entry {idx}: token must be exactly {TOKEN_LEN} characters");
            }
            if tokens.contains_key(name) {
                bail!("HCLOUD_TOKEN entry {idx}: duplicate name \"{name}\"");
            }
            if !seen_tokens.insert(token.to_string()) {
                bail!("HCLOUD_TOKEN entry {idx}: duplicate token");
            }
            tokens.insert(name.to_string(), token.to_string());
        }
    }
    if let Some(p) = &pin
        && !tokens.contains_key(p)
    {
        bail!("HCLOUD_PROJECT names a project that is not configured: \"{p}\"");
    }
    Ok(Projects { tokens, pin })
}

/// Inject the `project` schema property (§6.3) into every tool except
/// `list_projects`, which takes none. `required` mirrors "no pin configured"
/// (D2). Caveat: the property exists in the JSON schema only, never in the
/// Rust arg structs (T15) - safe today because no struct denies unknown
/// fields and rmcp does not validate arguments against the schema itself.
fn inject_project_property(router: &mut ToolRouter<HcloudServer>, required: bool) {
    for route in router.map.values_mut() {
        if route.attr.name == "list_projects" {
            continue;
        }
        let schema = Arc::make_mut(&mut route.attr.input_schema);
        schema
            .entry("properties".to_string())
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .expect("tool input schema \"properties\" must be a JSON object")
            .insert(
                "project".to_string(),
                serde_json::json!({"type": "string", "description": "Target project name."}),
            );
        if required {
            schema
                .entry("required".to_string())
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .expect("tool input schema \"required\" must be a JSON array")
                .push(serde_json::json!("project"));
        }
    }
}

/// MCP server wrapping an [`HcloudClient`]; the client's token is swapped
/// per call to the resolved project's (§7.1), so `client` always holds
/// whichever project was used most recently for construction only.
#[derive(Clone)]
pub struct HcloudServer {
    pub(crate) client: HcloudClient,
    pub(crate) projects: Arc<Projects>,
    pub(crate) tool_router: Arc<ToolRouter<Self>>,
}

impl HcloudServer {
    pub(crate) fn new(client: HcloudClient, projects: Projects) -> Self {
        let mut tool_router = Self::compute_router()
            + Self::infra_router()
            + Self::net_router()
            + Self::misc_router()
            + Self::servers_ops_router()
            + Self::res_ops_router()
            + Self::netres_ops_router()
            + Self::lb_zone_ops_router()
            + Self::projects_router();
        if projects.len() > 1 {
            inject_project_property(&mut tool_router, projects.pin.is_none());
        }
        Self {
            client,
            projects: Arc::new(projects),
            tool_router: Arc::new(tool_router),
        }
    }
}

/// Pull the `project` selector out of a call's arguments before dispatch
/// (§7.1) - the underlying arg structs never see it (§6.3 caveat, T15).
fn extract_selector(
    arguments: &mut Option<serde_json::Map<String, Value>>,
) -> Result<Option<String>, ErrorData> {
    match arguments.as_mut().and_then(|a| a.remove("project")) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ErrorData::invalid_params("project must be a string", None)),
    }
}

/// Prefix a tool's result with the resolved project name (§7.2, decision D1):
/// a JSON text payload is wrapped as `{"project", "result"}`; anything else
/// gets a leading `project: <name>` text block instead. Never touches a token.
fn annotate_project(result: CallToolResponse, name: &str) -> CallToolResponse {
    let CallToolResponse::Complete(mut result) = result else {
        return result;
    };
    if let Some(ContentBlock::Text(text)) = result.content.first() {
        match serde_json::from_str::<Value>(&text.text) {
            Ok(v) => {
                result.content[0] = ContentBlock::text(
                    serde_json::json!({"project": name, "result": v}).to_string(),
                );
            }
            Err(_) => {
                result
                    .content
                    .insert(0, ContentBlock::text(format!("project: {name}")));
            }
        }
    }
    CallToolResponse::Complete(result)
}

#[tool_router(router = projects_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "List every configured Hetzner project by name, with a cheap \
            fingerprint (server count and up to two server names) to catch a \
            mislabeled token. Never returns a token.",
        annotations(
            title = "List projects",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_projects(&self) -> Result<CallToolResult, ErrorData> {
        let pin = self.projects.pin.clone();
        let single = self.projects.len() == 1;
        let mut probes = Vec::new();
        for (name, token) in &self.projects.tokens {
            let client = self.client.with_token(token.clone());
            let name = name.clone();
            probes.push(tokio::spawn(async move {
                let fingerprint = match client
                    .get("/servers", &[("per_page", "2".to_string())])
                    .await
                {
                    Ok(v) => project_fingerprint(&v),
                    Err(e) => format!("unreachable: {e:#}"),
                };
                (name, fingerprint)
            }));
        }
        let mut projects = Vec::new();
        for probe in probes {
            let (name, fingerprint) = probe
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let is_default = single || pin.as_deref() == Some(name.as_str());
            projects.push(serde_json::json!({
                "name": name,
                "is_default": is_default,
                "fingerprint": fingerprint,
            }));
        }
        respond(Ok(serde_json::json!({ "projects": projects })))
    }
}

/// Cheap per-project fingerprint from `GET /servers?per_page=2`: total server
/// count plus up to two server names, so a mislabeled token is human-visible
/// (§9.1) even though no API call can name a token's project (C2).
fn project_fingerprint(v: &Value) -> String {
    let servers = v["servers"].as_array().cloned().unwrap_or_default();
    let total = v["meta"]["pagination"]["total_entries"]
        .as_u64()
        .unwrap_or(servers.len() as u64);
    let names: Vec<&str> = servers.iter().filter_map(|s| s["name"].as_str()).collect();
    if names.is_empty() {
        format!("{total} server(s)")
    } else {
        format!("{total} server(s): {}", names.join(", "))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for HcloudServer {
    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let selector = extract_selector(&mut request.arguments)?;
        let read_only = self
            .tool_router
            .get(request.name.as_ref())
            .and_then(|t| t.annotations.as_ref())
            .and_then(|a| a.read_only_hint)
            .unwrap_or(false);
        let (name, token) = self.projects.resolve(selector, read_only)?;
        let scoped = Self {
            client: self.client.with_token(token),
            ..self.clone()
        };
        let result = scoped
            .tool_router
            .call(ToolCallContext::new(&scoped, request, context))
            .await?;
        Ok(if self.projects.len() > 1 {
            annotate_project(result, &name)
        } else {
            result
        })
    }

    fn get_info(&self) -> ServerInfo {
        let mut instructions = String::from(
            "MCP server for the Hetzner Cloud API. Every list_*/get_* tool \
            is read-only and safe to call freely. Tools named create_*, \
            update_*, delete_*, power_server, and *_action mutate real \
            resources: creates may bill money, deletes are permanent, \
            actions can interrupt workloads - confirm with the user before \
            calling any of them.",
        );
        if self.projects.len() > 1 {
            instructions.push_str(&format!(
                " Several projects are configured; pass `project` on every tool \
                 call to select one (required unless the operator pinned a \
                 default). Configured projects: {}.",
                self.projects.names_list()
            ));
        }
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(instructions);
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
    let raw =
        std::env::var("HCLOUD_TOKEN").context("HCLOUD_TOKEN environment variable is required")?;
    let pin = std::env::var("HCLOUD_PROJECT")
        .ok()
        .filter(|s| !s.is_empty());
    let projects = parse_token_env(&raw, pin)?;
    let seed_token = projects
        .tokens
        .values()
        .next()
        .expect("parse_token_env always yields at least one project")
        .clone();
    let client = HcloudClient::from_env(seed_token)?;
    let service = HcloudServer::new(client, projects).serve(stdio()).await?;
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
            + HcloudServer::lb_zone_ops_router().list_all().len()
            + HcloudServer::projects_router().list_all().len();
        assert_eq!(server.tool_router.list_all().len(), parts);
        // Absolute pin: 13 compute + 10 infra + 8 net + 8 misc + 6 servers_ops
        // + 15 res_ops + 16 netres_ops + 16 lb_zone_ops + 1 list_projects.
        assert_eq!(server.tool_router.list_all().len(), 93);
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
        assert_eq!(titles.len(), 93);
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

/// Multi-project spec test matrix (T1-T16).
///
/// `call_tool` itself is ~10 lines of glue: `extract_selector`, then
/// `Projects::resolve`, then `client.with_token`, then dispatch. rmcp 3.1.4's
/// `Peer::new` is crate-private and this crate does not enable rmcp's
/// "client" feature (Cargo.toml is out of this brief's lease), so there is no
/// public way to build a `RequestContext` from outside the crate to drive
/// `call_tool` end-to-end. Each piece of that glue is instead proven directly:
/// `resolve`/`extract_selector` as pure functions, and the resolved token
/// reaching the wire via a manually-scoped client calling the real tool
/// method, exactly as `call_tool` does internally.
#[cfg(test)]
mod multi_project_tests {
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::ErrorCode;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::compute::ListServersArgs;
    use super::test_support::{project_token, server_for, server_for_projects};
    use super::*;

    /// Pre-feature baseline (measured at HEAD 47e590d, before this change):
    /// `serde_json::to_string(&server.tool_router.list_all())` for the
    /// original 92 tools was exactly this many bytes. §8/T1.
    const BASELINE_92_TOOLS_LEN: usize = 66_748;

    fn non_json_tools(tools: Vec<rmcp::model::Tool>) -> Vec<rmcp::model::Tool> {
        tools
            .into_iter()
            .filter(|t| t.name != "list_projects")
            .collect()
    }

    fn no_filters() -> ListServersArgs {
        ListServersArgs {
            name: None,
            label_selector: None,
            status: None,
            sort: None,
            page: None,
            per_page: None,
        }
    }

    /// Reproduce `call_tool`'s scoping step: clone `base` with its client
    /// pointed at `token`, exactly as the resolved (name, token) pair from
    /// `Projects::resolve` would be applied before dispatch.
    fn scoped(base: &HcloudServer, token: String) -> HcloudServer {
        HcloudServer {
            client: base.client.with_token(token),
            ..base.clone()
        }
    }

    // T1: single-project schemas carry no `project` property, and the
    // original 92 tools' serialized bytes match the pre-feature baseline.
    #[test]
    fn t1_single_project_schemas_are_unchanged() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let tools = non_json_tools(server.tool_router.list_all());
        assert_eq!(tools.len(), 92);
        for tool in &tools {
            assert!(
                !tool.input_schema.contains_key("project"),
                "{}: unexpected top-level project key",
                tool.name
            );
            if let Some(props) = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
            {
                assert!(!props.contains_key("project"), "{}: has project", tool.name);
            }
        }
        assert_eq!(
            serde_json::to_string(&tools).unwrap().len(),
            BASELINE_92_TOOLS_LEN
        );
    }

    // T2: multi-project schemas carry `project` on all 92 tools (not on
    // list_projects); required with no pin, optional with a pin.
    #[test]
    fn t2_multi_project_schemas_gain_the_project_property() {
        for (pin, required) in [(None, true), (Some("prod"), false)] {
            let server =
                server_for_projects("http://127.0.0.1:9".to_string(), &["prod", "staging"], pin);
            let tools = non_json_tools(server.tool_router.list_all());
            assert_eq!(tools.len(), 92);
            for tool in &tools {
                let props = tool.input_schema["properties"].as_object().unwrap();
                assert!(
                    props.contains_key("project"),
                    "{}: missing project",
                    tool.name
                );
                let is_required = tool
                    .input_schema
                    .get("required")
                    .is_some_and(|r| r.as_array().unwrap().iter().any(|v| v == "project"));
                assert_eq!(is_required, required, "{}: required mismatch", tool.name);
            }
        }
    }

    // T3: project: "staging" resolves to the staging token, and that token
    // (applied exactly as `call_tool` applies it) reaches the wire.
    #[tokio::test]
    async fn t3_routing_sends_the_named_projects_token() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .and(header(
                "authorization",
                format!("Bearer {}", project_token("staging")),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"servers": []})),
            )
            .mount(&mock)
            .await;

        let base = server_for_projects(mock.uri(), &["prod", "staging"], None);
        let (name, token) = base
            .projects
            .resolve(Some("staging".to_string()), false)
            .unwrap();
        assert_eq!(name, "staging");
        let result = scoped(&base, token)
            .list_servers(Parameters(no_filters()))
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true));
    }

    // T4: ambiguous call (n>1, no selector, no pin) is rejected. `resolve` is
    // a pure function with no HTTP client at all, so zero HTTP is inherent,
    // not merely observed.
    #[test]
    fn t4_ambiguous_call_is_rejected_with_zero_http() {
        let server =
            server_for_projects("http://127.0.0.1:9".to_string(), &["prod", "staging"], None);
        let e = server.projects.resolve(None, false).unwrap_err();
        assert_eq!(e.code, ErrorCode::INVALID_PARAMS);
        assert!(
            e.message.contains("prod") && e.message.contains("staging"),
            "{}",
            e.message
        );
    }

    // T5: an unknown project name is rejected before any HTTP request.
    #[test]
    fn t5_unknown_project_name_is_rejected_with_zero_http() {
        let server =
            server_for_projects("http://127.0.0.1:9".to_string(), &["prod", "staging"], None);
        let e = server
            .projects
            .resolve(Some("prodd".to_string()), false)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::INVALID_PARAMS);
        assert!(
            e.message.contains("prodd") && e.message.contains("prod, staging"),
            "{}",
            e.message
        );
    }

    // T6: an explicit selector in single-project mode, other than the
    // configured name, is rejected rather than silently ignored.
    #[test]
    fn t6_unknown_selector_in_single_project_mode_is_rejected_with_zero_http() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let e = server
            .projects
            .resolve(Some("prod".to_string()), false)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::INVALID_PARAMS);
        assert!(e.message.contains("default"), "{}", e.message);
    }

    // T6b: omitted, "default", and "" all succeed in single-project mode.
    #[test]
    fn t6b_accepted_selectors_in_single_project_mode_all_succeed() {
        let server = server_for("http://127.0.0.1:9".to_string());
        for selector in [None, Some("default".to_string()), Some(String::new())] {
            let (name, _token) = server.projects.resolve(selector, false).unwrap();
            assert_eq!(name, "default");
        }
    }

    // T6c: Form A and a one-entry Form B named "default" are indistinguishable.
    #[test]
    fn t6c_form_a_is_equivalent_to_an_explicit_default_entry() {
        let token = "a".repeat(64);
        let form_a = parse_token_env(&token, None).unwrap();
        let form_b = parse_token_env(&format!("default={token}"), None).unwrap();
        assert_eq!(form_a.tokens, form_b.tokens);
        assert_eq!(form_a.pin, form_b.pin);

        let a = HcloudServer::new(
            HcloudClient::new("http://127.0.0.1:9", token.clone()).unwrap(),
            form_a,
        );
        let b = HcloudServer::new(
            HcloudClient::new("http://127.0.0.1:9", token).unwrap(),
            form_b,
        );
        assert_eq!(
            serde_json::to_string(&a.tool_router.list_all()).unwrap(),
            serde_json::to_string(&b.tool_router.list_all()).unwrap()
        );
    }

    // T7: a pinned default is used when no selector is given, for a
    // read-only tool - and that token reaches the wire.
    #[tokio::test]
    async fn t7_pinned_default_is_used_for_a_read_only_tool() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .and(header(
                "authorization",
                format!("Bearer {}", project_token("prod")),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"servers": []})),
            )
            .mount(&mock)
            .await;

        let base = server_for_projects(mock.uri(), &["prod", "staging"], Some("prod"));
        let (name, token) = base.projects.resolve(None, true).unwrap();
        assert_eq!(name, "prod");
        let result = scoped(&base, token)
            .list_servers(Parameters(no_filters()))
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true));
    }

    // T8: the same pin does not excuse a mutating tool from an explicit
    // selector (§4 hardening, decision D2): `delete_server`'s read_only_hint
    // is false, so the pin must not apply.
    #[test]
    fn t8_pinned_default_does_not_cover_a_mutating_tool() {
        let server = server_for_projects(
            "http://127.0.0.1:9".to_string(),
            &["prod", "staging"],
            Some("prod"),
        );
        let e = server.projects.resolve(None, false).unwrap_err();
        assert_eq!(e.code, ErrorCode::INVALID_PARAMS);
    }

    // T9: every §3.2 rejection is a non-zero exit (an `Err`) whose message
    // never contains the offending token substring.
    #[test]
    fn t9_every_parse_rejection_omits_the_token() {
        let token = "b".repeat(64);
        let cases: Vec<(String, Option<&str>)> = vec![
            (format!("prod={token},"), None), // trailing comma: empty entry
            (format!("={token}"), None),      // empty name
            (format!("prod=,staging={token}"), None), // empty token
            ("prod=short".to_string(), None), // wrong length
            (format!("PROD={token}"), None),  // name outside [a-z0-9._-]
            (format!("prod={token},prod={token}"), None), // duplicate name
            (format!("prod={token},staging={token}"), None), // duplicate token
            (format!("prod={token},bare"), None), // one entry missing "name="
            (format!("prod={token}"), Some("elsewhere")), // HCLOUD_PROJECT names an unknown project
        ];
        for (raw, pin) in &cases {
            let err = parse_token_env(raw, pin.map(str::to_string)).unwrap_err();
            let msg = err.to_string();
            assert!(!msg.contains(&token), "leaked token in: {msg}");
        }
    }

    // T10: the spike's exact failure mode - a bare "=" reinterpreted as one
    // long token - is rejected by the 64-char guard, not silently truncated.
    #[test]
    fn t10_the_64_char_guard_rejects_the_spike_failure_mode() {
        // The spec's own example - rejected outright, never truncated to "BBBBBBBB".
        assert!(parse_token_env("AAAAAAAA=BBBBBBBB", None).is_err());
        // Isolate the length guard itself with an otherwise-valid name.
        let err = parse_token_env("prod=BBBBBBBB", None).unwrap_err();
        assert!(err.to_string().contains("64 characters"));
    }

    // T11: two names sharing one token is rejected at startup (a mislabel is
    // otherwise undetectable via the API, C2).
    #[test]
    fn t11_duplicate_token_is_rejected() {
        let token = "c".repeat(64);
        let err = parse_token_env(&format!("prod={token},staging={token}"), None).unwrap_err();
        assert!(err.to_string().contains("duplicate token"));
    }

    // T12: every result carries the resolved project name; the token never
    // appears in it - a JSON payload is wrapped, anything else gets a
    // leading text block (D1).
    #[test]
    fn t12_echo_wraps_a_json_result_with_the_project_name() {
        let result = ok_json(serde_json::json!({"servers": []})).unwrap();
        let CallToolResponse::Complete(wrapped) = annotate_project(result.into(), "staging") else {
            panic!("expected a complete result");
        };
        let text = wrapped.content[0].as_text().unwrap().text.clone();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            serde_json::json!({"project": "staging", "result": {"servers": []}})
        );
    }

    #[test]
    fn t12_echo_prepends_a_text_block_for_a_non_json_result() {
        let result = map_api_err(anyhow::anyhow!("boom"));
        let CallToolResponse::Complete(wrapped) = annotate_project(result.into(), "staging") else {
            panic!("expected a complete result");
        };
        assert_eq!(
            wrapped.content[0].as_text().unwrap().text,
            "project: staging"
        );
        assert!(wrapped.content[1].as_text().unwrap().text.contains("boom"));
    }

    // T13: get_pricing, the one tool with an empty `properties` object, still
    // resolves and routes correctly in multi-project mode.
    #[tokio::test]
    async fn t13_get_pricing_routes_correctly_in_multi_project_mode() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pricing"))
            .and(header(
                "authorization",
                format!("Bearer {}", project_token("staging")),
            ))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"pricing": {}})),
            )
            .mount(&mock)
            .await;

        let base = server_for_projects(mock.uri(), &["prod", "staging"], None);
        let (_, token) = base
            .projects
            .resolve(Some("staging".to_string()), true)
            .unwrap();
        let result = scoped(&base, token).get_pricing().await.unwrap();
        assert_ne!(result.is_error, Some(true));
    }

    // T14: list_projects names every project and whether it is the pinned
    // default; no token ever appears in the result.
    #[tokio::test]
    async fn t14_list_projects_reports_names_and_default_without_a_token() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .and(query_param("per_page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "servers": [{"name": "web-1"}],
                "meta": {"pagination": {"total_entries": 1}}
            })))
            .mount(&mock)
            .await;

        let server = server_for_projects(mock.uri(), &["prod", "staging"], Some("prod"));
        let result = server.list_projects().await.unwrap();
        let text = result.content[0].as_text().unwrap().text.clone();
        assert!(!text.contains(&project_token("prod")));
        assert!(!text.contains(&project_token("staging")));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let projects = v["projects"].as_array().unwrap();
        assert_eq!(projects.len(), 2);
        let prod = projects.iter().find(|p| p["name"] == "prod").unwrap();
        assert_eq!(prod["is_default"], true);
        let staging = projects.iter().find(|p| p["name"] == "staging").unwrap();
        assert_eq!(staging["is_default"], false);
    }

    // call_tool glue: extract_selector pulls "project" out and rejects a
    // non-string value before it ever reaches `Projects::resolve`.
    #[test]
    fn extract_selector_removes_project_and_rejects_non_strings() {
        let mut none: Option<serde_json::Map<String, Value>> = None;
        assert_eq!(extract_selector(&mut none).unwrap(), None);

        let mut with_project = Some(
            serde_json::json!({"id": 1, "project": "prod"})
                .as_object()
                .unwrap()
                .clone(),
        );
        assert_eq!(
            extract_selector(&mut with_project).unwrap(),
            Some("prod".to_string())
        );
        assert!(!with_project.unwrap().contains_key("project"));

        let mut bad = Some(
            serde_json::json!({"project": 5})
                .as_object()
                .unwrap()
                .clone(),
        );
        assert_eq!(
            extract_selector(&mut bad).unwrap_err().code,
            ErrorCode::INVALID_PARAMS
        );
    }

    // T15: schema/struct divergence guard (§6.3 caveat) - an argument object
    // carrying a field no struct declares still deserializes, because no
    // struct here uses `deny_unknown_fields`.
    #[test]
    fn t15_an_unexpected_extra_argument_does_not_break_deserialization() {
        let value = serde_json::json!({"id": 1, "project": "prod"});
        let parsed: IdArgs = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.id, 1);
    }
}
