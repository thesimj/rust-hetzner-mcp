//! Network/firewall/floating_ip/primary_ip mutations (M15c, brief B25).
//!
//! `subnets`/`routes`/`rules`/`apply_to` and action `params` are passed
//! through as raw JSON: each nests a distinct Hetzner sub-schema, and this
//! module's job is to forward what the caller sends, not re-model every
//! upstream shape as a parallel Rust struct.

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData, tool, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{HcloudServer, IdArgs, respond};

const NETWORK_ACTIONS: [&str; 6] = [
    "add_route",
    "add_subnet",
    "change_ip_range",
    "change_protection",
    "delete_route",
    "delete_subnet",
];
const FIREWALL_ACTIONS: [&str; 3] = ["apply_to_resources", "remove_from_resources", "set_rules"];
const FLOATING_IP_ACTIONS: [&str; 4] =
    ["assign", "unassign", "change_dns_ptr", "change_protection"];
const PRIMARY_IP_ACTIONS: [&str; 4] = ["assign", "unassign", "change_dns_ptr", "change_protection"];

/// Shape shared by all four `*_action` tools: an allowlisted action name
/// plus an optional passthrough body. The allowlist differs per tool and is
/// enforced in the tool body, not the schema.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ActionArgs {
    /// Numeric ID of the resource to act on.
    pub id: u64,
    /// Action name; see this tool's description for the allowed values.
    pub action: String,
    /// Action-specific request body, forwarded to the API as-is. Omit for
    /// actions that take no parameters (e.g. unassign).
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateNetworkArgs {
    /// Name for the new network. Must be unique per project.
    pub name: String,
    /// IP range of the whole network in CIDR notation, e.g. "10.0.0.0/16".
    pub ip_range: String,
    /// Subnets to create along with the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnets: Option<Vec<Value>>,
    /// Routes to create along with the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routes: Option<Vec<Value>>,
    /// Whether routes from this network are exposed to the vSwitch connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expose_routes_to_vswitch: Option<bool>,
    /// Labels to attach to the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateNetworkArgs {
    /// Numeric ID of the network to update.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New name for the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Labels to set on the network (replaces the current set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
    /// Whether routes from this network are exposed to the vSwitch connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expose_routes_to_vswitch: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateFirewallArgs {
    /// Name for the new firewall. Must be unique per project.
    pub name: String,
    /// Firewall rules to create along with the firewall.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<Value>>,
    /// Resources to apply the firewall to immediately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_to: Option<Vec<Value>>,
    /// Labels to attach to the firewall.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateFirewallArgs {
    /// Numeric ID of the firewall to update.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New name for the firewall.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Labels to set on the firewall (replaces the current set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateFloatingIpArgs {
    /// Floating IP type: "ipv4" or "ipv6".
    pub r#type: String,
    /// Home location name or ID. Only optional if `server` is given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home_location: Option<String>,
    /// Server ID to assign the Floating IP to immediately.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<u64>,
    /// Description of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Name for the resource. Must be unique per project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Labels to attach to the Floating IP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateFloatingIpArgs {
    /// Numeric ID of the Floating IP to update.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New description for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Labels to set on the Floating IP (replaces the current set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
    /// New name for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreatePrimaryIpArgs {
    /// Name for the resource. Must be unique per project.
    pub name: String,
    /// Primary IP type: "ipv4" or "ipv6".
    pub r#type: String,
    /// Type of resource to assign the Primary IP to: "server" is the only
    /// accepted value. Omit to leave unassigned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_type: Option<String>,
    /// ID of the resource to assign the Primary IP to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<u64>,
    /// Location name or ID to bind the Primary IP to. Omit if `assignee_id`/
    /// `assignee_type` are given. The current API spec has no separate
    /// datacenter-level placement field for this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Whether to delete the Primary IP once its assigned resource is deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_delete: Option<bool>,
    /// Labels to attach to the Primary IP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdatePrimaryIpArgs {
    /// Numeric ID of the Primary IP to update.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New name for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whether to delete the Primary IP once its assigned resource is deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_delete: Option<bool>,
    /// Labels to set on the Primary IP (replaces the current set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[tool_router(router = netres_ops_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "Create a private network. Networks themselves are free; only \
        the resources you attach to them (e.g. servers) are billed.",
        annotations(
            title = "Create network",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_network(
        &self,
        Parameters(args): Parameters<CreateNetworkArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/networks", body).await)
    }

    #[tool(
        description = "Update a network's name, labels, or vSwitch route exposure.",
        annotations(
            title = "Update network",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_network(
        &self,
        Parameters(args): Parameters<UpdateNetworkArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.put(&format!("/networks/{id}"), body).await)
    }

    #[tool(
        description = "Delete a network permanently.",
        annotations(
            title = "Delete network",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_network(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/networks/{id}")).await)
    }

    #[tool(
        description = "Run an action on a network: add_route, add_subnet, \
        change_ip_range, change_protection, delete_route, or delete_subnet. \
        `params` carries the action's body, e.g. {\"destination\":..,\"gateway\":..} \
        for add_route or {\"ip_range\":..} for change_ip_range.",
        annotations(
            title = "Network action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn network_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if !NETWORK_ACTIONS.contains(&args.action.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "action must be one of {}, got {:?}",
                    NETWORK_ACTIONS.join(", "),
                    args.action
                ),
                None,
            ));
        }
        respond(
            self.client
                .post(
                    &format!("/networks/{}/actions/{}", args.id, args.action),
                    args.params.unwrap_or_else(|| serde_json::json!({})),
                )
                .await,
        )
    }

    #[tool(
        description = "Create a firewall that can be applied to servers. Firewalls \
        themselves are free; only the servers they protect are billed.",
        annotations(
            title = "Create firewall",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_firewall(
        &self,
        Parameters(args): Parameters<CreateFirewallArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/firewalls", body).await)
    }

    #[tool(
        description = "Update a firewall's name or labels.",
        annotations(
            title = "Update firewall",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_firewall(
        &self,
        Parameters(args): Parameters<UpdateFirewallArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.put(&format!("/firewalls/{id}"), body).await)
    }

    #[tool(
        description = "Delete a firewall permanently.",
        annotations(
            title = "Delete firewall",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_firewall(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/firewalls/{id}")).await)
    }

    #[tool(
        description = "Run an action on a firewall: apply_to_resources, \
        remove_from_resources, or set_rules. set_rules REPLACES the firewall's \
        entire rule set with `params.rules`, not just adds to it.",
        annotations(
            title = "Firewall action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn firewall_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if !FIREWALL_ACTIONS.contains(&args.action.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "action must be one of {}, got {:?}",
                    FIREWALL_ACTIONS.join(", "),
                    args.action
                ),
                None,
            ));
        }
        respond(
            self.client
                .post(
                    &format!("/firewalls/{}/actions/{}", args.id, args.action),
                    args.params.unwrap_or_else(|| serde_json::json!({})),
                )
                .await,
        )
    }

    #[tool(
        description = "Create a Floating IP. This creates a BILLABLE resource.",
        annotations(
            title = "Create floating IP",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_floating_ip(
        &self,
        Parameters(args): Parameters<CreateFloatingIpArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/floating_ips", body).await)
    }

    #[tool(
        description = "Update a Floating IP's description, labels, or name.",
        annotations(
            title = "Update floating IP",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_floating_ip(
        &self,
        Parameters(args): Parameters<UpdateFloatingIpArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.put(&format!("/floating_ips/{id}"), body).await)
    }

    #[tool(
        description = "Delete a Floating IP permanently.",
        annotations(
            title = "Delete floating IP",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_floating_ip(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/floating_ips/{id}")).await)
    }

    #[tool(
        description = "Run an action on a Floating IP: assign, unassign, \
        change_dns_ptr, or change_protection.",
        annotations(
            title = "Floating IP action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn floating_ip_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if !FLOATING_IP_ACTIONS.contains(&args.action.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "action must be one of {}, got {:?}",
                    FLOATING_IP_ACTIONS.join(", "),
                    args.action
                ),
                None,
            ));
        }
        respond(
            self.client
                .post(
                    &format!("/floating_ips/{}/actions/{}", args.id, args.action),
                    args.params.unwrap_or_else(|| serde_json::json!({})),
                )
                .await,
        )
    }

    #[tool(
        description = "Create a Primary IP. This creates a BILLABLE resource.",
        annotations(
            title = "Create primary IP",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_primary_ip(
        &self,
        Parameters(args): Parameters<CreatePrimaryIpArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/primary_ips", body).await)
    }

    #[tool(
        description = "Update a Primary IP's name, auto_delete flag, or labels.",
        annotations(
            title = "Update primary IP",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_primary_ip(
        &self,
        Parameters(args): Parameters<UpdatePrimaryIpArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.put(&format!("/primary_ips/{id}"), body).await)
    }

    #[tool(
        description = "Delete a Primary IP permanently.",
        annotations(
            title = "Delete primary IP",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_primary_ip(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/primary_ips/{id}")).await)
    }

    #[tool(
        description = "Run an action on a Primary IP: assign, unassign, \
        change_dns_ptr, or change_protection.",
        annotations(
            title = "Primary IP action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn primary_ip_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if !PRIMARY_IP_ACTIONS.contains(&args.action.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "action must be one of {}, got {:?}",
                    PRIMARY_IP_ACTIONS.join(", "),
                    args.action
                ),
                None,
            ));
        }
        respond(
            self.client
                .post(
                    &format!("/primary_ips/{}/actions/{}", args.id, args.action),
                    args.params.unwrap_or_else(|| serde_json::json!({})),
                )
                .await,
        )
    }
}

#[cfg(test)]
mod tests {
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::ErrorCode;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::server::test_support::{server_for, tool_result_json};

    #[tokio::test]
    async fn create_network_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/networks"))
            .and(body_json(serde_json::json!({
                "name": "net-1", "ip_range": "10.0.0.0/16"
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"network": {"id": 1}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_network(Parameters(CreateNetworkArgs {
                name: "net-1".into(),
                ip_range: "10.0.0.0/16".into(),
                subnets: None,
                routes: None,
                expose_routes_to_vswitch: None,
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"network": {"id": 1}})
        );
    }

    /// Sets every optional field, including the passthrough arrays, so a
    /// rename of any field name (subnets/routes included) shows up as a
    /// body mismatch rather than a silently-dropped field.
    #[tokio::test]
    async fn create_network_sends_every_field_when_all_optionals_are_set() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/networks"))
            .and(body_json(serde_json::json!({
                "name": "net-1",
                "ip_range": "10.0.0.0/16",
                "subnets": [{"type": "cloud", "ip_range": "10.0.1.0/24", "network_zone": "eu-central"}],
                "routes": [{"destination": "10.100.1.0/24", "gateway": "10.0.1.1"}],
                "expose_routes_to_vswitch": true,
                "labels": {"env": "prod"}
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"network": {"id": 1}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_network(Parameters(CreateNetworkArgs {
                name: "net-1".into(),
                ip_range: "10.0.0.0/16".into(),
                subnets: Some(vec![serde_json::json!({
                    "type": "cloud", "ip_range": "10.0.1.0/24", "network_zone": "eu-central"
                })]),
                routes: Some(vec![
                    serde_json::json!({"destination": "10.100.1.0/24", "gateway": "10.0.1.1"}),
                ]),
                expose_routes_to_vswitch: Some(true),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"network": {"id": 1}})
        );
    }

    /// Sets every optional field so a rename of any of them shows up as a
    /// body mismatch, not a silently-dropped field; also proves `id` is
    /// excluded from the body regardless of what else is set.
    #[tokio::test]
    async fn update_network_excludes_id_and_sends_every_other_field_when_set() {
        let mock = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/networks/5"))
            .and(body_json(serde_json::json!({
                "name": "renamed",
                "labels": {"env": "prod"},
                "expose_routes_to_vswitch": true
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"network": {"id": 5}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_network(Parameters(UpdateNetworkArgs {
                id: 5,
                name: Some("renamed".into()),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
                expose_routes_to_vswitch: Some(true),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"network": {"id": 5}})
        );
    }

    #[tokio::test]
    async fn create_firewall_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/firewalls"))
            .and(body_json(serde_json::json!({"name": "fw-1"})))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"firewall": {"id": 1}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_firewall(Parameters(CreateFirewallArgs {
                name: "fw-1".into(),
                rules: None,
                apply_to: None,
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"firewall": {"id": 1}})
        );
    }

    /// Sets every optional field, including the passthrough arrays; see
    /// create_network's twin for why.
    #[tokio::test]
    async fn create_firewall_sends_every_field_when_all_optionals_are_set() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/firewalls"))
            .and(body_json(serde_json::json!({
                "name": "fw-1",
                "rules": [{
                    "direction": "in", "protocol": "tcp", "port": "80",
                    "source_ips": ["0.0.0.0/0"]
                }],
                "apply_to": [{"type": "label_selector", "label_selector": "env=prod"}],
                "labels": {"env": "prod"}
            })))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"firewall": {"id": 1}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_firewall(Parameters(CreateFirewallArgs {
                name: "fw-1".into(),
                rules: Some(vec![serde_json::json!({
                    "direction": "in", "protocol": "tcp", "port": "80",
                    "source_ips": ["0.0.0.0/0"]
                })]),
                apply_to: Some(vec![
                    serde_json::json!({"type": "label_selector", "label_selector": "env=prod"}),
                ]),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"firewall": {"id": 1}})
        );
    }

    /// Sets every optional field; see update_network's twin for why.
    #[tokio::test]
    async fn update_firewall_excludes_id_and_sends_every_other_field_when_set() {
        let mock = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/firewalls/3"))
            .and(body_json(serde_json::json!({
                "name": "renamed",
                "labels": {"env": "prod"}
            })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"firewall": {"id": 3}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_firewall(Parameters(UpdateFirewallArgs {
                id: 3,
                name: Some("renamed".into()),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"firewall": {"id": 3}})
        );
    }

    #[tokio::test]
    async fn create_floating_ip_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/floating_ips"))
            .and(body_json(serde_json::json!({"type": "ipv4"})))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "floating_ip": {"id": 1}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_floating_ip(Parameters(CreateFloatingIpArgs {
                r#type: "ipv4".into(),
                home_location: None,
                server: None,
                description: None,
                name: None,
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"floating_ip": {"id": 1}})
        );
    }

    /// Sets every optional field; see create_network's twin for why.
    #[tokio::test]
    async fn create_floating_ip_sends_every_field_when_all_optionals_are_set() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/floating_ips"))
            .and(body_json(serde_json::json!({
                "type": "ipv4",
                "home_location": "fsn1",
                "server": 42,
                "description": "my desc",
                "name": "fip-1",
                "labels": {"env": "prod"}
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "floating_ip": {"id": 1}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_floating_ip(Parameters(CreateFloatingIpArgs {
                r#type: "ipv4".into(),
                home_location: Some("fsn1".into()),
                server: Some(42),
                description: Some("my desc".into()),
                name: Some("fip-1".into()),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"floating_ip": {"id": 1}})
        );
    }

    /// Sets every optional field; see update_network's twin for why.
    #[tokio::test]
    async fn update_floating_ip_excludes_id_and_sends_every_other_field_when_set() {
        let mock = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/floating_ips/6"))
            .and(body_json(serde_json::json!({
                "description": "new desc",
                "labels": {"env": "prod"},
                "name": "renamed"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "floating_ip": {"id": 6}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_floating_ip(Parameters(UpdateFloatingIpArgs {
                id: 6,
                description: Some("new desc".into()),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
                name: Some("renamed".into()),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"floating_ip": {"id": 6}})
        );
    }

    #[tokio::test]
    async fn create_primary_ip_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/primary_ips"))
            .and(body_json(serde_json::json!({
                "name": "pip-1", "type": "ipv4"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "primary_ip": {"id": 1}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_primary_ip(Parameters(CreatePrimaryIpArgs {
                name: "pip-1".into(),
                r#type: "ipv4".into(),
                assignee_type: None,
                assignee_id: None,
                location: None,
                auto_delete: None,
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"primary_ip": {"id": 1}})
        );
    }

    /// Sets every optional field; see create_network's twin for why.
    #[tokio::test]
    async fn create_primary_ip_sends_every_field_when_all_optionals_are_set() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/primary_ips"))
            .and(body_json(serde_json::json!({
                "name": "pip-1",
                "type": "ipv4",
                "assignee_type": "server",
                "assignee_id": 42,
                "location": "fsn1",
                "auto_delete": true,
                "labels": {"env": "prod"}
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "primary_ip": {"id": 1}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_primary_ip(Parameters(CreatePrimaryIpArgs {
                name: "pip-1".into(),
                r#type: "ipv4".into(),
                assignee_type: Some("server".into()),
                assignee_id: Some(42),
                location: Some("fsn1".into()),
                auto_delete: Some(true),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"primary_ip": {"id": 1}})
        );
    }

    /// Sets every optional field; see update_network's twin for why.
    #[tokio::test]
    async fn update_primary_ip_excludes_id_and_sends_every_other_field_when_set() {
        let mock = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/primary_ips/2"))
            .and(body_json(serde_json::json!({
                "name": "renamed",
                "auto_delete": true,
                "labels": {"env": "prod"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "primary_ip": {"id": 2}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_primary_ip(Parameters(UpdatePrimaryIpArgs {
                id: 2,
                name: Some("renamed".into()),
                auto_delete: Some(true),
                labels: Some(BTreeMap::from([("env".to_string(), "prod".to_string())])),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"primary_ip": {"id": 2}})
        );
    }

    /// One row per group: delete_* hits `/{resource}/{id}` and returns the
    /// envelope untouched.
    #[tokio::test]
    async fn delete_tools_hit_their_id_path_and_return_the_envelope() {
        let mock = MockServer::start().await;
        for route in [
            "/networks/9",
            "/firewalls/4",
            "/floating_ips/8",
            "/primary_ips/11",
        ] {
            Mock::given(method("DELETE"))
                .and(path(route))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"action": {}})),
                )
                .mount(&mock)
                .await;
        }

        let server = server_for(mock.uri());
        for res in [
            server
                .delete_network(Parameters(IdArgs { id: 9 }))
                .await
                .unwrap(),
            server
                .delete_firewall(Parameters(IdArgs { id: 4 }))
                .await
                .unwrap(),
            server
                .delete_floating_ip(Parameters(IdArgs { id: 8 }))
                .await
                .unwrap(),
            server
                .delete_primary_ip(Parameters(IdArgs { id: 11 }))
                .await
                .unwrap(),
        ] {
            assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
        }
    }

    /// One row per action tool with `params` set (the None -> `{}` default
    /// is covered by every row of the allowlist-coverage test below).
    #[tokio::test]
    async fn action_tools_post_params_to_the_action_path() {
        let mock = MockServer::start().await;
        for (route, body) in [
            (
                "/networks/9/actions/change_ip_range",
                serde_json::json!({"ip_range": "10.0.0.0/24"}),
            ),
            (
                "/firewalls/4/actions/set_rules",
                serde_json::json!({"rules": []}),
            ),
            (
                "/floating_ips/8/actions/assign",
                serde_json::json!({"server": 42}),
            ),
            (
                "/primary_ips/11/actions/assign",
                serde_json::json!({"assignee_type": "server", "assignee_id": 42}),
            ),
        ] {
            Mock::given(method("POST"))
                .and(path(route))
                .and(body_json(body))
                .respond_with(
                    ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
                )
                .mount(&mock)
                .await;
        }

        let server = server_for(mock.uri());
        for res in [
            server
                .network_action(Parameters(ActionArgs {
                    id: 9,
                    action: "change_ip_range".into(),
                    params: Some(serde_json::json!({"ip_range": "10.0.0.0/24"})),
                }))
                .await
                .unwrap(),
            server
                .firewall_action(Parameters(ActionArgs {
                    id: 4,
                    action: "set_rules".into(),
                    params: Some(serde_json::json!({"rules": []})),
                }))
                .await
                .unwrap(),
            server
                .floating_ip_action(Parameters(ActionArgs {
                    id: 8,
                    action: "assign".into(),
                    params: Some(serde_json::json!({"server": 42})),
                }))
                .await
                .unwrap(),
            server
                .primary_ip_action(Parameters(ActionArgs {
                    id: 11,
                    action: "assign".into(),
                    params: Some(serde_json::json!({"assignee_type": "server", "assignee_id": 42})),
                }))
                .await
                .unwrap(),
        ] {
            assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
        }
    }

    /// Pins each group's allowlist to its exact expected array (catches a
    /// garbled or cross-wired const), then proves every allowlisted name
    /// reaches its own `/{group}/{id}/actions/{name}` path with `{}`.
    #[tokio::test]
    async fn action_tools_reach_every_allowlisted_action_path() {
        assert_eq!(
            NETWORK_ACTIONS,
            [
                "add_route",
                "add_subnet",
                "change_ip_range",
                "change_protection",
                "delete_route",
                "delete_subnet"
            ]
        );
        assert_eq!(
            FIREWALL_ACTIONS,
            ["apply_to_resources", "remove_from_resources", "set_rules"]
        );
        assert_eq!(
            FLOATING_IP_ACTIONS,
            ["assign", "unassign", "change_dns_ptr", "change_protection"]
        );
        assert_eq!(
            PRIMARY_IP_ACTIONS,
            ["assign", "unassign", "change_dns_ptr", "change_protection"]
        );

        let mock = MockServer::start().await;
        for (group, actions) in [
            ("networks", &NETWORK_ACTIONS[..]),
            ("firewalls", &FIREWALL_ACTIONS[..]),
            ("floating_ips", &FLOATING_IP_ACTIONS[..]),
            ("primary_ips", &PRIMARY_IP_ACTIONS[..]),
        ] {
            for action in actions {
                Mock::given(method("POST"))
                    .and(path(format!("/{group}/1/actions/{action}")))
                    .and(body_json(serde_json::json!({})))
                    .respond_with(
                        ResponseTemplate::new(201)
                            .set_body_json(serde_json::json!({"action": {"command": action}})),
                    )
                    .mount(&mock)
                    .await;
            }
        }

        // Same call shape per group, only the tool method differs; a macro
        // says that once instead of four near-identical loop bodies.
        macro_rules! assert_reaches_every_action {
            ($server:expr, $method:ident, $actions:expr) => {
                for action in $actions {
                    let res = $server
                        .$method(Parameters(ActionArgs {
                            id: 1,
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
            };
        }
        let server = server_for(mock.uri());
        assert_reaches_every_action!(server, network_action, NETWORK_ACTIONS);
        assert_reaches_every_action!(server, firewall_action, FIREWALL_ACTIONS);
        assert_reaches_every_action!(server, floating_ip_action, FLOATING_IP_ACTIONS);
        assert_reaches_every_action!(server, primary_ip_action, PRIMARY_IP_ACTIONS);
    }

    /// One row per action tool: an action outside its allowlist is rejected
    /// with INVALID_PARAMS before any request is sent (base URL has nothing
    /// listening on it).
    #[tokio::test]
    async fn action_tools_reject_unknown_actions_with_invalid_params() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let bad = |id| ActionArgs {
            id,
            action: "nuke".into(),
            params: None,
        };
        macro_rules! assert_rejects_bad_action {
            ($server:expr, $method:ident) => {
                assert_eq!(
                    $server.$method(Parameters(bad(1))).await.unwrap_err().code,
                    ErrorCode::INVALID_PARAMS
                );
            };
        }
        assert_rejects_bad_action!(server, network_action);
        assert_rejects_bad_action!(server, firewall_action);
        assert_rejects_bad_action!(server, floating_ip_action);
        assert_rejects_bad_action!(server, primary_ip_action);
    }

    /// Mirrors compute's/infra's router annotation assertion: (read_only,
    /// destructive) per tool, so flipping a hint on any of the 16 tools
    /// breaks the suite.
    #[test]
    fn netres_ops_router_registers_all_16_tools_with_expected_annotations() {
        let router = super::HcloudServer::netres_ops_router();
        let expected: [(&str, bool, bool); 16] = [
            ("create_network", false, false),
            ("update_network", false, false),
            ("delete_network", false, true),
            ("network_action", false, true),
            ("create_firewall", false, false),
            ("update_firewall", false, false),
            ("delete_firewall", false, true),
            ("firewall_action", false, true),
            ("create_floating_ip", false, false),
            ("update_floating_ip", false, false),
            ("delete_floating_ip", false, true),
            ("floating_ip_action", false, true),
            ("create_primary_ip", false, false),
            ("update_primary_ip", false, false),
            ("delete_primary_ip", false, true),
            ("primary_ip_action", false, true),
        ];
        assert_eq!(router.list_all().len(), 16);
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
        }
    }
}
