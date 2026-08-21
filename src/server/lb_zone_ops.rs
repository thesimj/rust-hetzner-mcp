//! Load balancer and DNS zone (+rrset) mutations (M15d, brief B26).
//!
//! Zone `id_or_name` and rrset `name`/`type` are string path segments, so
//! each is validated before interpolation instead of relying on the
//! client's blanket `..`/`?`/`#` guard (which does not catch e.g. embedded
//! whitespace or a bare `/`).

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{
    HcloudServer, IdArgs, MAX_ZONE_ID_LEN, ZoneIdArgs, pagination_query, push_param,
    require_update_fields, respond, validate_zone_id,
};

const LB_ACTIONS: [&str; 13] = [
    "add_service",
    "add_target",
    "attach_to_network",
    "change_algorithm",
    "change_dns_ptr",
    "change_protection",
    "change_type",
    "delete_service",
    "detach_from_network",
    "disable_public_interface",
    "enable_public_interface",
    "remove_target",
    "update_service",
];

const ZONE_ACTIONS: [&str; 4] = [
    "change_primary_nameservers",
    "change_protection",
    "change_ttl",
    "import_zonefile",
];

const METRIC_TYPES: [&str; 4] = [
    "open_connections",
    "connections_per_second",
    "requests_per_second",
    "bandwidth",
];

const RRSET_ACTIONS: [&str; 6] = [
    "change_protection",
    "change_ttl",
    "set_records",
    "add_records",
    "remove_records",
    "update_records",
];

fn check_action(allowed: &[&str], action: &str) -> Result<(), ErrorData> {
    if allowed.contains(&action) {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "action must be one of {}, got {:?}",
                allowed.join(", "),
                action
            ),
            None,
        ))
    }
}

/// Turn an action's optional params object into a POST body - `{}` when unset.
fn action_body(params: Option<serde_json::Map<String, serde_json::Value>>) -> serde_json::Value {
    params.map_or_else(|| serde_json::json!({}), serde_json::Value::Object)
}

/// RRSet `name` path segment: non-empty, at most [`MAX_ZONE_ID_LEN`] chars,
/// not exactly ".", no ".." substring, `[A-Za-z0-9._@*-]`. Same hole as
/// `validate_zone_id`: a bare "." collapses the URL, sliding `rr_type` into
/// the `rr_name` position.
fn validate_rrset_name(name: &str) -> Result<(), ErrorData> {
    let ok = !name.is_empty()
        && name.len() <= MAX_ZONE_ID_LEN
        && name != "."
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._@*-".contains(c));
    if ok {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "rrset name must be non-empty, at most {MAX_ZONE_ID_LEN} chars, not \".\", not \
                 contain \"..\", and contain only [A-Za-z0-9._@*-], got {name:?}"
            ),
            None,
        ))
    }
}

/// RRSet `type` path segment: non-empty, `[A-Z0-9]`.
fn validate_rrset_type(value: &str) -> Result<(), ErrorData> {
    let ok = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!("rrset type must be non-empty and contain only [A-Z0-9], got {value:?}"),
            None,
        ))
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateLoadBalancerArgs {
    /// Name of the new Load Balancer.
    pub name: String,
    /// ID or name of the Load Balancer type, e.g. "lb11".
    pub load_balancer_type: String,
    /// Algorithm config, e.g. `{"type": "round_robin"}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<serde_json::Value>,
    /// ID or name of the Location to create the Load Balancer in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Name of the network zone, e.g. "eu-central".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_zone: Option<String>,
    /// ID of the network to attach the Load Balancer to on creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<u64>,
    /// Array of service definitions, per the Hetzner Load Balancer service schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<serde_json::Value>,
    /// Array of target definitions, per the Hetzner Load Balancer target schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<serde_json::Value>,
    /// Labels to attach to the Load Balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
    /// Enable or disable the public interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_interface: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateLoadBalancerArgs {
    /// ID of the Load Balancer to update.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New name for the Load Balancer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Labels to overwrite the existing set with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LoadBalancerActionArgs {
    /// ID of the Load Balancer to act on.
    pub id: u64,
    /// Action name; see the tool description for the allowed set.
    pub action: String,
    /// Action-specific parameters object; omit for actions that take no
    /// parameters. A JSON object (not `serde_json::Value`), so a bare string
    /// or number can never become the POST body.
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetLoadBalancerMetricsArgs {
    /// ID of the Load Balancer.
    pub id: u64,
    /// Metric type(s): open_connections, connections_per_second, requests_per_second, bandwidth.
    #[serde(rename = "type")]
    pub r#type: Vec<String>,
    /// Start of the time range (RFC 3339 timestamp).
    pub start: String,
    /// End of the time range (RFC 3339 timestamp).
    pub end: String,
    /// Resolution of the returned data points, in seconds.
    pub step: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateZoneArgs {
    /// Name of the Zone, e.g. "example.com".
    pub name: String,
    /// Zone mode: "primary" or "secondary".
    pub mode: String,
    /// Default TTL of the Zone, in seconds (60 - 2147483647).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 60))]
    pub ttl: Option<u32>,
    /// Zone file content to seed the Zone with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zonefile: Option<String>,
    /// Labels to attach to the Zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
    /// Primary nameservers (required for mode "secondary" to be completable;
    /// ignored for "primary"). Each entry is `{"address": ..., "port"?: ...}`
    /// per the Hetzner spec; can also be set later via zone_action's
    /// change_primary_nameservers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_nameservers: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateZoneArgs {
    /// ID or name of the Zone to update.
    #[serde(skip_serializing)]
    pub id_or_name: String,
    /// Labels to overwrite the existing set with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ZoneActionArgs {
    /// ID or name of the Zone to act on.
    pub id_or_name: String,
    /// Action name; see the tool description for the allowed set.
    pub action: String,
    /// Action-specific parameters object; omit for actions that take no
    /// parameters. A JSON object (not `serde_json::Value`), so a bare string
    /// or number can never become the POST body.
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListZoneRrsetsArgs {
    /// ID or name of the Zone.
    pub id_or_name: String,
    /// Exact RRSet name to filter by.
    pub name: Option<String>,
    /// Filter by RRSet type; repeatable.
    #[serde(rename = "type")]
    pub r#type: Option<Vec<String>>,
    /// Label selector, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Sort order, e.g. "name:asc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Page number, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page (default 25; the spec sets no maximum).
    #[schemars(range(min = 1))]
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ZoneRrsetIdArgs {
    /// ID or name of the Zone.
    pub id_or_name: String,
    /// Name of the RRSet, e.g. "www" (use "@" for the zone apex).
    pub rr_name: String,
    /// Type of the RRSet, e.g. "A".
    pub rr_type: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct RrsetRecord {
    /// Value of the record.
    pub value: String,
    /// Optional comment for the record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateZoneRrsetArgs {
    /// ID or name of the Zone to create the RRSet in.
    #[serde(skip_serializing)]
    pub id_or_name: String,
    /// Name of the RRSet, e.g. "www" (use "@" for the zone apex).
    pub name: String,
    /// Type of the RRSet, e.g. "A".
    #[serde(rename = "type")]
    pub r#type: String,
    /// TTL of the RRSet, in seconds; the Zone's default TTL is used if unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 60))]
    pub ttl: Option<u32>,
    /// Records of the RRSet; must be non-empty.
    pub records: Vec<RrsetRecord>,
    /// Labels to attach to the RRSet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateZoneRrsetArgs {
    /// ID or name of the Zone the RRSet belongs to.
    #[serde(skip_serializing)]
    pub id_or_name: String,
    /// Name of the RRSet to update.
    #[serde(skip_serializing)]
    pub rr_name: String,
    /// Type of the RRSet to update.
    #[serde(skip_serializing)]
    pub rr_type: String,
    /// Labels to overwrite the existing set with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ZoneRrsetActionArgs {
    /// ID or name of the Zone the RRSet belongs to.
    pub id_or_name: String,
    /// Name of the RRSet to act on.
    pub rr_name: String,
    /// Type of the RRSet to act on.
    pub rr_type: String,
    /// Action name; see the tool description for the allowed set.
    pub action: String,
    /// Action-specific parameters object; omit for actions that take no
    /// parameters. A JSON object (not `serde_json::Value`), so a bare string
    /// or number can never become the POST body.
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
}

#[tool_router(router = lb_zone_ops_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "Create a new Load Balancer. This creates a BILLABLE resource.",
        annotations(
            title = "Create Load Balancer",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_load_balancer(
        &self,
        Parameters(args): Parameters<CreateLoadBalancerArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/load_balancers", body).await)
    }

    #[tool(
        description = "Update a Load Balancer's name and/or labels.",
        annotations(
            title = "Update Load Balancer",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_load_balancer(
        &self,
        Parameters(args): Parameters<UpdateLoadBalancerArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let path = format!("/load_balancers/{}", args.id);
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&path, body).await)
    }

    #[tool(
        description = "Delete a Load Balancer permanently.",
        annotations(
            title = "Delete Load Balancer",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_load_balancer(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/load_balancers/{id}")).await)
    }

    #[tool(
        description = "Run an action on a Load Balancer. action must be one of: add_service, \
        add_target, attach_to_network, change_algorithm, change_dns_ptr, change_protection, \
        change_type, delete_service, detach_from_network, disable_public_interface, \
        enable_public_interface, remove_target, update_service. Several of these interrupt \
        traffic or change routing.",
        annotations(
            title = "Load Balancer action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn load_balancer_action(
        &self,
        Parameters(args): Parameters<LoadBalancerActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        check_action(&LB_ACTIONS, &args.action)?;
        respond(
            self.client
                .post(
                    &format!("/load_balancers/{}/actions/{}", args.id, args.action),
                    action_body(args.params),
                )
                .await,
        )
    }

    #[tool(
        description = "Get time-series metrics for a Load Balancer.",
        annotations(
            title = "Get Load Balancer metrics",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_load_balancer_metrics(
        &self,
        Parameters(args): Parameters<GetLoadBalancerMetricsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.r#type.is_empty() {
            return Err(ErrorData::invalid_params(
                "type must contain at least one metric type",
                None,
            ));
        }
        for t in &args.r#type {
            if !METRIC_TYPES.contains(&t.as_str()) {
                return Err(ErrorData::invalid_params(
                    format!(
                        "type must be one of {}, got {:?}",
                        METRIC_TYPES.join(", "),
                        t
                    ),
                    None,
                ));
            }
        }
        let mut query = vec![("start", args.start), ("end", args.end)];
        for t in args.r#type {
            query.push(("type", t));
        }
        push_param(&mut query, "step", args.step);
        respond(
            self.client
                .get(&format!("/load_balancers/{}/metrics", args.id), &query)
                .await,
        )
    }

    #[tool(
        description = "Create a new DNS Zone. A mode \"secondary\" Zone needs \
        primary_nameservers set here, or afterwards via zone_action's \
        change_primary_nameservers, to be functional.",
        annotations(
            title = "Create Zone",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_zone(
        &self,
        Parameters(args): Parameters<CreateZoneArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/zones", body).await)
    }

    #[tool(
        description = "Update a Zone's labels.",
        annotations(
            title = "Update Zone",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_zone(
        &self,
        Parameters(args): Parameters<UpdateZoneArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        let path = format!("/zones/{}", args.id_or_name);
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&path, body).await)
    }

    #[tool(
        description = "Delete a Zone permanently, including all its RRSets.",
        annotations(
            title = "Delete Zone",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_zone(
        &self,
        Parameters(ZoneIdArgs { id_or_name }): Parameters<ZoneIdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&id_or_name)?;
        respond(self.client.delete(&format!("/zones/{id_or_name}")).await)
    }

    #[tool(
        description = "Run an action on a Zone. action must be one of: \
        change_primary_nameservers, change_protection, change_ttl, import_zonefile.",
        annotations(
            title = "Zone action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn zone_action(
        &self,
        Parameters(args): Parameters<ZoneActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        check_action(&ZONE_ACTIONS, &args.action)?;
        respond(
            self.client
                .post(
                    &format!("/zones/{}/actions/{}", args.id_or_name, args.action),
                    action_body(args.params),
                )
                .await,
        )
    }

    #[tool(
        description = "List the RRSets of a Zone, optionally filtered by name, type, or label selector.",
        annotations(
            title = "List Zone RRSets",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_zone_rrsets(
        &self,
        Parameters(args): Parameters<ListZoneRrsetsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        let mut query = pagination_query(args.page, args.per_page);
        push_param(&mut query, "name", args.name);
        push_param(&mut query, "label_selector", args.label_selector);
        for t in args.r#type.into_iter().flatten() {
            push_param(&mut query, "type", Some(t));
        }
        for s in args.sort.into_iter().flatten() {
            push_param(&mut query, "sort", Some(s));
        }
        respond(
            self.client
                .get(&format!("/zones/{}/rrsets", args.id_or_name), &query)
                .await,
        )
    }

    #[tool(
        description = "Get a single RRSet by zone and name/type.",
        annotations(
            title = "Get Zone RRSet",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_zone_rrset(
        &self,
        Parameters(args): Parameters<ZoneRrsetIdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        validate_rrset_name(&args.rr_name)?;
        validate_rrset_type(&args.rr_type)?;
        respond(
            self.client
                .get(
                    &format!(
                        "/zones/{}/rrsets/{}/{}",
                        args.id_or_name, args.rr_name, args.rr_type
                    ),
                    &[],
                )
                .await,
        )
    }

    #[tool(
        description = "Create a new RRSet in a Zone.",
        annotations(
            title = "Create Zone RRSet",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_zone_rrset(
        &self,
        Parameters(args): Parameters<CreateZoneRrsetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        if args.records.is_empty() {
            return Err(ErrorData::invalid_params("records must not be empty", None));
        }
        let path = format!("/zones/{}/rrsets", args.id_or_name);
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post(&path, body).await)
    }

    #[tool(
        description = "Update an RRSet's labels.",
        annotations(
            title = "Update Zone RRSet",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_zone_rrset(
        &self,
        Parameters(args): Parameters<UpdateZoneRrsetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        validate_rrset_name(&args.rr_name)?;
        validate_rrset_type(&args.rr_type)?;
        let path = format!(
            "/zones/{}/rrsets/{}/{}",
            args.id_or_name, args.rr_name, args.rr_type
        );
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&path, body).await)
    }

    #[tool(
        description = "Delete an RRSet from a Zone permanently.",
        annotations(
            title = "Delete Zone RRSet",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_zone_rrset(
        &self,
        Parameters(args): Parameters<ZoneRrsetIdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        validate_rrset_name(&args.rr_name)?;
        validate_rrset_type(&args.rr_type)?;
        respond(
            self.client
                .delete(&format!(
                    "/zones/{}/rrsets/{}/{}",
                    args.id_or_name, args.rr_name, args.rr_type
                ))
                .await,
        )
    }

    #[tool(
        description = "Run an action on an RRSet: add_records, change_protection, \
        change_ttl, remove_records, set_records, update_records. set_records and \
        update_records REPLACE data rather than merge it.",
        annotations(
            title = "Zone RRSet action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn zone_rrset_action(
        &self,
        Parameters(args): Parameters<ZoneRrsetActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        validate_rrset_name(&args.rr_name)?;
        validate_rrset_type(&args.rr_type)?;
        check_action(&RRSET_ACTIONS, &args.action)?;
        respond(
            self.client
                .post(
                    &format!(
                        "/zones/{}/rrsets/{}/{}/actions/{}",
                        args.id_or_name, args.rr_name, args.rr_type, args.action
                    ),
                    action_body(args.params),
                )
                .await,
        )
    }
}

#[cfg(test)]
mod tests {
    use rmcp::model::ErrorCode;
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::server::test_support::{server_for, tool_result_json};

    /// `*_action` `params` is object-typed (N2); tests build it from a JSON
    /// object literal for readability.
    fn map_of(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().expect("object literal").clone()
    }

    /// Values that must never reach a URL path: traversal, an extra segment,
    /// a query-string char, empty, embedded whitespace, a bare/embedded
    /// "..", and a too-long value (254 > the 253 DNS-length bound).
    fn hostile_strings() -> Vec<String> {
        let mut v: Vec<String> = ["../x", "a/b", "A?", "", "TXT injection", ".", ".."]
            .into_iter()
            .map(String::from)
            .collect();
        v.push("a".repeat(254));
        v
    }

    /// A dead port: if the tool under test skips validation and attempts a
    /// request, `respond()` turns the transport failure into `Ok(isError)`,
    /// so `.unwrap_err()` below only succeeds when validation short-circuited.
    fn dead_server() -> HcloudServer {
        server_for("http://127.0.0.1:9".to_string())
    }

    /// Asserts a tool call rejects before any request is attempted (see `dead_server`).
    macro_rules! assert_invalid_params {
        ($call:expr) => {
            assert_eq!($call.await.unwrap_err().code, ErrorCode::INVALID_PARAMS);
        };
    }

    #[tokio::test]
    async fn create_load_balancer_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/load_balancers"))
            .and(body_json(serde_json::json!({
                "name": "web-lb",
                "load_balancer_type": "lb11"
            })))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"load_balancer": {"id": 1}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_load_balancer(Parameters(CreateLoadBalancerArgs {
                name: "web-lb".into(),
                load_balancer_type: "lb11".into(),
                algorithm: None,
                location: None,
                network_zone: None,
                network: None,
                services: None,
                targets: None,
                labels: None,
                public_interface: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"load_balancer": {"id": 1}})
        );
    }

    #[tokio::test]
    async fn create_load_balancer_forwards_optional_fields() {
        let mock = MockServer::start().await;
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        Mock::given(method("POST"))
            .and(path("/load_balancers"))
            .and(body_json(serde_json::json!({
                "name": "web-lb",
                "load_balancer_type": "lb11",
                "algorithm": {"type": "least_connections"},
                "location": "fsn1",
                "network_zone": "eu-central",
                "network": 42,
                "services": [{"protocol": "tcp", "listen_port": 80}],
                "targets": [{"type": "server", "server": {"id": 1}}],
                "labels": {"env": "prod"},
                "public_interface": true
            })))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"load_balancer": {"id": 2}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_load_balancer(Parameters(CreateLoadBalancerArgs {
                name: "web-lb".into(),
                load_balancer_type: "lb11".into(),
                algorithm: Some(serde_json::json!({"type": "least_connections"})),
                location: Some("fsn1".into()),
                network_zone: Some("eu-central".into()),
                network: Some(42),
                services: Some(serde_json::json!([{"protocol": "tcp", "listen_port": 80}])),
                targets: Some(serde_json::json!([{"type": "server", "server": {"id": 1}}])),
                labels: Some(labels),
                public_interface: Some(true),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"load_balancer": {"id": 2}})
        );
    }

    #[tokio::test]
    async fn update_load_balancer_sends_only_the_provided_fields() {
        let mock = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/load_balancers/5"))
            .and(body_json(serde_json::json!({"name": "new-name"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"load_balancer": {"id": 5}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_load_balancer(Parameters(UpdateLoadBalancerArgs {
                id: 5,
                name: Some("new-name".into()),
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"load_balancer": {"id": 5}})
        );
    }

    #[tokio::test]
    async fn delete_load_balancer_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/load_balancers/7"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_load_balancer(Parameters(IdArgs { id: 7 }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn load_balancer_action_posts_params_to_the_action_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/load_balancers/7/actions/change_algorithm"))
            .and(body_json(serde_json::json!({"type": "least_connections"})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .load_balancer_action(Parameters(LoadBalancerActionArgs {
                id: 7,
                action: "change_algorithm".into(),
                params: Some(map_of(serde_json::json!({"type": "least_connections"}))),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn load_balancer_action_defaults_to_an_empty_body_when_params_omitted() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/load_balancers/7/actions/enable_public_interface"))
            .and(body_json(serde_json::json!({})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .load_balancer_action(Parameters(LoadBalancerActionArgs {
                id: 7,
                action: "enable_public_interface".into(),
                params: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn load_balancer_action_rejects_unknown_actions_with_invalid_params() {
        let err = dead_server()
            .load_balancer_action(Parameters(LoadBalancerActionArgs {
                id: 7,
                action: "nuke".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    /// N2 schema-level proof: `params` is object-typed on both action arg
    /// structs, so a bare scalar fails to deserialize instead of becoming
    /// the HTTP body.
    #[test]
    fn action_params_reject_a_scalar_at_the_schema_level() {
        assert!(
            serde_json::from_value::<LoadBalancerActionArgs>(serde_json::json!({
                "id": 1, "action": "change_algorithm", "params": "nuke"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ZoneActionArgs>(serde_json::json!({
                "id_or_name": "example.com", "action": "change_ttl", "params": 42
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn get_load_balancer_metrics_passes_repeated_type_and_range_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/load_balancers/9/metrics"))
            .and(query_param("type", "open_connections"))
            .and(query_param("type", "bandwidth"))
            .and(query_param("start", "2024-01-01T00:00:00Z"))
            .and(query_param("end", "2024-01-02T00:00:00Z"))
            .and(query_param("step", "60"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"metrics": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .get_load_balancer_metrics(Parameters(GetLoadBalancerMetricsArgs {
                id: 9,
                r#type: vec!["open_connections".into(), "bandwidth".into()],
                start: "2024-01-01T00:00:00Z".into(),
                end: "2024-01-02T00:00:00Z".into(),
                step: Some("60".into()),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"metrics": {}}));
    }

    #[tokio::test]
    async fn get_load_balancer_metrics_rejects_unknown_type_with_invalid_params() {
        let err = dead_server()
            .get_load_balancer_metrics(Parameters(GetLoadBalancerMetricsArgs {
                id: 9,
                r#type: vec!["cpu".into()],
                start: "2024-01-01T00:00:00Z".into(),
                end: "2024-01-02T00:00:00Z".into(),
                step: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn get_load_balancer_metrics_rejects_an_empty_type_list() {
        let err = dead_server()
            .get_load_balancer_metrics(Parameters(GetLoadBalancerMetricsArgs {
                id: 9,
                r#type: vec![],
                start: "2024-01-01T00:00:00Z".into(),
                end: "2024-01-02T00:00:00Z".into(),
                step: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn create_zone_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/zones"))
            .and(body_json(serde_json::json!({
                "name": "example.com",
                "mode": "primary"
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"zone": {"id": "1"}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_zone(Parameters(CreateZoneArgs {
                name: "example.com".into(),
                mode: "primary".into(),
                ttl: None,
                zonefile: None,
                labels: None,
                primary_nameservers: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"zone": {"id": "1"}})
        );
    }

    #[tokio::test]
    async fn create_zone_forwards_optional_fields() {
        let mock = MockServer::start().await;
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        Mock::given(method("POST"))
            .and(path("/zones"))
            .and(body_json(serde_json::json!({
                "name": "example.com",
                "mode": "secondary",
                "ttl": 10800,
                "zonefile": "$ORIGIN example.com.",
                "labels": {"env": "prod"},
                "primary_nameservers": [{"address": "198.51.100.1"}]
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"zone": {"id": "2"}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_zone(Parameters(CreateZoneArgs {
                name: "example.com".into(),
                mode: "secondary".into(),
                ttl: Some(10800),
                zonefile: Some("$ORIGIN example.com.".into()),
                labels: Some(labels),
                primary_nameservers: Some(vec![serde_json::json!({"address": "198.51.100.1"})]),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"zone": {"id": "2"}})
        );
    }

    #[tokio::test]
    async fn update_zone_sends_the_labels_body() {
        let mock = MockServer::start().await;
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "staging".to_string());
        Mock::given(method("PUT"))
            .and(path("/zones/example.com"))
            .and(body_json(serde_json::json!({"labels": {"env": "staging"}})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"zone": {"id": "1"}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_zone(Parameters(UpdateZoneArgs {
                id_or_name: "example.com".into(),
                labels: Some(labels),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"zone": {"id": "1"}})
        );
    }

    #[tokio::test]
    async fn delete_zone_hits_the_id_or_name_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/zones/example.com"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_zone(Parameters(ZoneIdArgs {
                id_or_name: "example.com".into(),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn zone_action_posts_params_to_the_action_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/zones/example.com/actions/change_ttl"))
            .and(body_json(serde_json::json!({"ttl": 3600})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .zone_action(Parameters(ZoneActionArgs {
                id_or_name: "example.com".into(),
                action: "change_ttl".into(),
                params: Some(map_of(serde_json::json!({"ttl": 3600}))),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn zone_action_rejects_unknown_actions_with_invalid_params() {
        let err = dead_server()
            .zone_action(Parameters(ZoneActionArgs {
                id_or_name: "example.com".into(),
                action: "export_zonefile".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn list_zone_rrsets_passes_filters_as_repeated_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zones/example.com/rrsets"))
            .and(query_param("name", "www"))
            .and(query_param("type", "A"))
            .and(query_param("type", "AAAA"))
            .and(query_param("label_selector", "env=prod"))
            .and(query_param("sort", "name:asc"))
            .and(query_param("page", "2"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"rrsets": []})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .list_zone_rrsets(Parameters(ListZoneRrsetsArgs {
                id_or_name: "example.com".into(),
                name: Some("www".into()),
                r#type: Some(vec!["A".into(), "AAAA".into()]),
                label_selector: Some("env=prod".into()),
                sort: Some(vec!["name:asc".into()]),
                page: Some(2),
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"rrsets": []}));
    }

    #[tokio::test]
    async fn get_zone_rrset_hits_the_path() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zones/example.com/rrsets/www/A"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"rrset": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .get_zone_rrset(Parameters(ZoneRrsetIdArgs {
                id_or_name: "example.com".into(),
                rr_name: "www".into(),
                rr_type: "A".into(),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"rrset": {}}));
    }

    #[tokio::test]
    async fn create_zone_rrset_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/zones/example.com/rrsets"))
            .and(body_json(serde_json::json!({
                "name": "www",
                "type": "A",
                "records": [{"value": "198.51.100.1"}]
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"rrset": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_zone_rrset(Parameters(CreateZoneRrsetArgs {
                id_or_name: "example.com".into(),
                name: "www".into(),
                r#type: "A".into(),
                ttl: None,
                records: vec![RrsetRecord {
                    value: "198.51.100.1".into(),
                    comment: None,
                }],
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"rrset": {}}));
    }

    #[tokio::test]
    async fn create_zone_rrset_forwards_optional_fields() {
        let mock = MockServer::start().await;
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        Mock::given(method("POST"))
            .and(path("/zones/example.com/rrsets"))
            .and(body_json(serde_json::json!({
                "name": "www",
                "type": "A",
                "ttl": 3600,
                "records": [{"value": "198.51.100.1", "comment": "web"}],
                "labels": {"env": "prod"}
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"rrset": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_zone_rrset(Parameters(CreateZoneRrsetArgs {
                id_or_name: "example.com".into(),
                name: "www".into(),
                r#type: "A".into(),
                ttl: Some(3600),
                records: vec![RrsetRecord {
                    value: "198.51.100.1".into(),
                    comment: Some("web".into()),
                }],
                labels: Some(labels),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"rrset": {}}));
    }

    #[tokio::test]
    async fn create_zone_rrset_rejects_empty_records() {
        let err = dead_server()
            .create_zone_rrset(Parameters(CreateZoneRrsetArgs {
                id_or_name: "example.com".into(),
                name: "www".into(),
                r#type: "A".into(),
                ttl: None,
                records: vec![],
                labels: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn update_zone_rrset_sends_the_labels_body() {
        let mock = MockServer::start().await;
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        Mock::given(method("PUT"))
            .and(path("/zones/example.com/rrsets/www/A"))
            .and(body_json(serde_json::json!({"labels": {"env": "prod"}})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"rrset": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_zone_rrset(Parameters(UpdateZoneRrsetArgs {
                id_or_name: "example.com".into(),
                rr_name: "www".into(),
                rr_type: "A".into(),
                labels: Some(labels),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"rrset": {}}));
    }

    #[tokio::test]
    async fn delete_zone_rrset_hits_the_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/zones/example.com/rrsets/www/A"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"success": true})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_zone_rrset(Parameters(ZoneRrsetIdArgs {
                id_or_name: "example.com".into(),
                rr_name: "www".into(),
                rr_type: "A".into(),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"success": true}));
    }

    /// Every tool that interpolates `id_or_name` into a path must reject a
    /// hostile value before any request is attempted (dead-port proof).
    #[tokio::test]
    async fn zone_id_validation_guards_every_tool_that_interpolates_it() {
        let bad = "a/b".to_string();
        let server = dead_server();

        assert_invalid_params!(server.update_zone(Parameters(UpdateZoneArgs {
            id_or_name: bad.clone(),
            labels: None
        })));
        assert_invalid_params!(server.delete_zone(Parameters(ZoneIdArgs {
            id_or_name: bad.clone()
        })));
        assert_invalid_params!(server.zone_action(Parameters(ZoneActionArgs {
            id_or_name: bad.clone(),
            action: "change_ttl".into(),
            params: None
        })));
        assert_invalid_params!(server.list_zone_rrsets(Parameters(ListZoneRrsetsArgs {
            id_or_name: bad.clone(),
            name: None,
            r#type: None,
            label_selector: None,
            sort: None,
            page: None,
            per_page: None
        })));
        assert_invalid_params!(server.get_zone_rrset(Parameters(ZoneRrsetIdArgs {
            id_or_name: bad.clone(),
            rr_name: "www".into(),
            rr_type: "A".into()
        })));
        assert_invalid_params!(server.create_zone_rrset(Parameters(CreateZoneRrsetArgs {
            id_or_name: bad.clone(),
            name: "www".into(),
            r#type: "A".into(),
            ttl: None,
            records: vec![RrsetRecord {
                value: "198.51.100.1".into(),
                comment: None
            }],
            labels: None
        })));
        assert_invalid_params!(server.update_zone_rrset(Parameters(UpdateZoneRrsetArgs {
            id_or_name: bad.clone(),
            rr_name: "www".into(),
            rr_type: "A".into(),
            labels: None
        })));
        assert_invalid_params!(server.delete_zone_rrset(Parameters(ZoneRrsetIdArgs {
            id_or_name: bad.clone(),
            rr_name: "www".into(),
            rr_type: "A".into()
        })));
        assert_invalid_params!(server.zone_rrset_action(Parameters(ZoneRrsetActionArgs {
            id_or_name: bad,
            rr_name: "www".into(),
            rr_type: "A".into(),
            action: "change_ttl".into(),
            params: None
        })));
    }

    /// Every hostile value in the brief's rejection set must be rejected as
    /// a zone `id_or_name`, proven via the dead-port technique.
    #[tokio::test]
    async fn zone_id_rejects_every_hostile_value() {
        let server = dead_server();
        for bad in hostile_strings() {
            let err = server
                .delete_zone(Parameters(ZoneIdArgs {
                    id_or_name: bad.clone(),
                }))
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "value {bad:?}");
        }
    }

    /// Both rrset path segments (`rr_name`, `rr_type`) must reject a hostile
    /// value at every tool that interpolates them into a path.
    #[tokio::test]
    async fn rrset_name_and_type_validation_guards_every_tool_that_interpolates_them() {
        let bad = "a/b".to_string();
        let server = dead_server();

        assert_invalid_params!(server.get_zone_rrset(Parameters(ZoneRrsetIdArgs {
            id_or_name: "example.com".into(),
            rr_name: bad.clone(),
            rr_type: "A".into()
        })));
        assert_invalid_params!(server.get_zone_rrset(Parameters(ZoneRrsetIdArgs {
            id_or_name: "example.com".into(),
            rr_name: "www".into(),
            rr_type: bad.clone()
        })));
        assert_invalid_params!(server.update_zone_rrset(Parameters(UpdateZoneRrsetArgs {
            id_or_name: "example.com".into(),
            rr_name: bad.clone(),
            rr_type: "A".into(),
            labels: None
        })));
        assert_invalid_params!(server.update_zone_rrset(Parameters(UpdateZoneRrsetArgs {
            id_or_name: "example.com".into(),
            rr_name: "www".into(),
            rr_type: bad.clone(),
            labels: None
        })));
        assert_invalid_params!(server.delete_zone_rrset(Parameters(ZoneRrsetIdArgs {
            id_or_name: "example.com".into(),
            rr_name: bad.clone(),
            rr_type: "A".into()
        })));
        assert_invalid_params!(server.delete_zone_rrset(Parameters(ZoneRrsetIdArgs {
            id_or_name: "example.com".into(),
            rr_name: "www".into(),
            rr_type: bad.clone()
        })));
        assert_invalid_params!(server.zone_rrset_action(Parameters(ZoneRrsetActionArgs {
            id_or_name: "example.com".into(),
            rr_name: bad.clone(),
            rr_type: "A".into(),
            action: "change_ttl".into(),
            params: None
        })));
        assert_invalid_params!(server.zone_rrset_action(Parameters(ZoneRrsetActionArgs {
            id_or_name: "example.com".into(),
            rr_name: "www".into(),
            rr_type: bad,
            action: "change_ttl".into(),
            params: None
        })));
    }

    /// Every hostile value in the brief's rejection set must be rejected as
    /// both an rrset name and an rrset type.
    #[tokio::test]
    async fn get_zone_rrset_rejects_every_hostile_name_and_type_value() {
        let server = dead_server();
        for bad in hostile_strings() {
            let err = server
                .get_zone_rrset(Parameters(ZoneRrsetIdArgs {
                    id_or_name: "example.com".into(),
                    rr_name: bad.clone(),
                    rr_type: "A".into(),
                }))
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "name {bad:?}");

            let err = server
                .get_zone_rrset(Parameters(ZoneRrsetIdArgs {
                    id_or_name: "example.com".into(),
                    rr_name: "www".into(),
                    rr_type: bad.clone(),
                }))
                .await
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::INVALID_PARAMS, "type {bad:?}");
        }
    }

    /// Mirrors compute.rs's router annotation assertion: (read_only,
    /// destructive) per tool, so flipping a hint on any of the 15 tools
    /// breaks the suite. Also asserts open_world_hint and distinct,
    /// non-empty titles (N6: parity with net.rs/misc.rs/res_ops.rs).
    #[test]
    fn lb_zone_ops_router_registers_all_15_tools_with_expected_annotations() {
        let router = super::HcloudServer::lb_zone_ops_router();
        let expected: [(&str, bool, bool); 15] = [
            ("create_load_balancer", false, false),
            ("update_load_balancer", false, false),
            ("delete_load_balancer", false, true),
            ("load_balancer_action", false, true),
            ("get_load_balancer_metrics", true, false),
            ("create_zone", false, false),
            ("update_zone", false, false),
            ("delete_zone", false, true),
            ("zone_action", false, true),
            ("list_zone_rrsets", true, false),
            ("get_zone_rrset", true, false),
            ("create_zone_rrset", false, false),
            ("update_zone_rrset", false, false),
            ("delete_zone_rrset", false, true),
            ("zone_rrset_action", false, true),
        ];
        assert_eq!(router.list_all().len(), 15);
        let mut titles = std::collections::HashSet::new();
        for (name, read_only, destructive) in expected {
            let tool = router
                .get(name)
                .unwrap_or_else(|| panic!("missing route: {name}"));
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{name} has no annotations"));
            assert_eq!(
                annotations.read_only_hint,
                Some(read_only),
                "{name} must be read_only_hint = {read_only}"
            );
            assert_eq!(
                annotations.destructive_hint,
                Some(destructive),
                "{name} must be destructive_hint = {destructive}"
            );
            assert_eq!(
                annotations.open_world_hint,
                Some(true),
                "{name} must be open_world_hint = true"
            );
            let title = annotations
                .title
                .clone()
                .unwrap_or_else(|| panic!("{name} has no title"));
            assert!(!title.is_empty(), "{name} title must not be empty");
            assert!(
                titles.insert(title.clone()),
                "title {title:?} reused by more than one tool"
            );
        }
        assert_eq!(titles.len(), 15, "every tool must have a distinct title");
    }

    /// Pins every allowlist/enum literally, not just "one bad value gets
    /// rejected" - a garbled or truncated entry elsewhere in the array
    /// would otherwise pass the other tests unnoticed.
    #[test]
    fn action_and_metric_allowlists_match_the_spec_exactly() {
        assert_eq!(
            LB_ACTIONS,
            [
                "add_service",
                "add_target",
                "attach_to_network",
                "change_algorithm",
                "change_dns_ptr",
                "change_protection",
                "change_type",
                "delete_service",
                "detach_from_network",
                "disable_public_interface",
                "enable_public_interface",
                "remove_target",
                "update_service",
            ]
        );
        assert_eq!(
            ZONE_ACTIONS,
            [
                "change_primary_nameservers",
                "change_protection",
                "change_ttl",
                "import_zonefile",
            ]
        );
        assert_eq!(
            METRIC_TYPES,
            [
                "open_connections",
                "connections_per_second",
                "requests_per_second",
                "bandwidth",
            ]
        );
        assert_eq!(
            RRSET_ACTIONS,
            [
                "change_protection",
                "change_ttl",
                "set_records",
                "add_records",
                "remove_records",
                "update_records",
            ]
        );
    }

    /// N9 happy path: every allowlisted rrset action reaches its own
    /// `/zones/{id_or_name}/rrsets/{name}/{type}/actions/{action}` path with
    /// the params object forwarded as the POST body.
    #[tokio::test]
    async fn zone_rrset_action_reaches_every_allowlisted_action_path() {
        let mock = MockServer::start().await;
        for action in RRSET_ACTIONS {
            Mock::given(method("POST"))
                .and(path(format!(
                    "/zones/example.com/rrsets/www/A/actions/{action}"
                )))
                .and(body_json(serde_json::json!({})))
                .respond_with(
                    ResponseTemplate::new(201)
                        .set_body_json(serde_json::json!({"action": {"command": action}})),
                )
                .mount(&mock)
                .await;
        }

        let server = server_for(mock.uri());
        for action in RRSET_ACTIONS {
            let res = server
                .zone_rrset_action(Parameters(ZoneRrsetActionArgs {
                    id_or_name: "example.com".into(),
                    rr_name: "www".into(),
                    rr_type: "A".into(),
                    action: action.into(),
                    params: None,
                }))
                .await
                .unwrap();
            assert_eq!(
                tool_result_json(&res)["action"]["command"],
                serde_json::json!(action)
            );
        }
    }

    #[tokio::test]
    async fn zone_rrset_action_posts_params_to_the_action_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/zones/example.com/rrsets/www/A/actions/set_records"))
            .and(body_json(serde_json::json!({
                "records": [{"value": "198.51.100.1"}]
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .zone_rrset_action(Parameters(ZoneRrsetActionArgs {
                id_or_name: "example.com".into(),
                rr_name: "www".into(),
                rr_type: "A".into(),
                action: "set_records".into(),
                params: Some(map_of(
                    serde_json::json!({"records": [{"value": "198.51.100.1"}]}),
                )),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn zone_rrset_action_rejects_an_action_outside_the_allowlist() {
        let err = dead_server()
            .zone_rrset_action(Parameters(ZoneRrsetActionArgs {
                id_or_name: "example.com".into(),
                rr_name: "www".into(),
                rr_type: "A".into(),
                action: "delete_zonefile".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    /// The BILLABLE warning and the full 13-action list must actually be in
    /// the wire-visible tool description, not just in a code comment.
    #[test]
    fn create_load_balancer_and_load_balancer_action_descriptions_are_complete() {
        let router = super::HcloudServer::lb_zone_ops_router();
        let create_desc = router
            .get("create_load_balancer")
            .unwrap()
            .description
            .clone()
            .unwrap_or_default();
        assert!(create_desc.contains("BILLABLE"), "got: {create_desc}");

        let action_desc = router
            .get("load_balancer_action")
            .unwrap()
            .description
            .clone()
            .unwrap_or_default();
        for action in LB_ACTIONS {
            assert!(
                action_desc.contains(action),
                "load_balancer_action description missing {action:?}: {action_desc}"
            );
        }
    }
}
