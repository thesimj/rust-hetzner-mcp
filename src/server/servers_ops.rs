//! Server mutations and metrics, global actions, pricing (M15a, brief B23).

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{HcloudServer, IdArgs, push_param, require_update_fields, respond};

/// The full set of `POST /servers/{id}/actions/*` commands in the spec.
/// power_server already covers poweron/poweroff/reboot/shutdown; this tool
/// exists for the other 19 plus anything scripted through the same path.
const SERVER_ACTIONS: [&str; 23] = [
    "add_to_placement_group",
    "attach_iso",
    "attach_to_network",
    "change_alias_ips",
    "change_dns_ptr",
    "change_protection",
    "change_type",
    "create_image",
    "detach_from_network",
    "detach_iso",
    "disable_backup",
    "disable_rescue",
    "enable_backup",
    "enable_rescue",
    "poweroff",
    "poweron",
    "reboot",
    "rebuild",
    "remove_from_placement_group",
    "request_console",
    "reset",
    "reset_password",
    "shutdown",
];

const METRIC_TYPES: [&str; 3] = ["cpu", "disk", "network"];

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateServerArgs {
    /// Server ID to update; sent in the path, not the request body.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New name to set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Labels to set; replaces the full existing set, not a merge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GetServerMetricsArgs {
    /// Server ID to get metrics for.
    pub id: u64,
    /// Metric type(s) to fetch: cpu, disk, network; repeatable.
    #[serde(rename = "type")]
    pub r#type: Vec<String>,
    /// Start of the period to fetch (RFC3339).
    pub start: String,
    /// End of the period to fetch (RFC3339).
    pub end: String,
    /// Resolution of results in seconds; the API picks one if omitted.
    pub step: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ServerActionArgs {
    /// Server ID to act on.
    pub id: u64,
    /// Action name; must be one of the actions the Hetzner Cloud API exposes
    /// under POST /servers/{id}/actions/*.
    pub action: String,
    /// Per-action request body (a JSON object), e.g. {"image": "ubuntu-24.04"}
    /// for rebuild. Sent as {} when omitted - correct for body-less actions.
    pub params: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListActionsArgs {
    /// Action IDs to fetch. The API removed listing all actions (2025-01-30);
    /// an empty list is rejected, not sent as a bare GET /actions.
    pub id: Vec<u64>,
}

#[tool_router(router = servers_ops_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "Update a server's name and/or labels. Labels replace \
        the full existing set, not a merge.",
        annotations(
            title = "Update server",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_server(
        &self,
        Parameters(args): Parameters<UpdateServerArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&format!("/servers/{id}"), body).await)
    }

    #[tool(
        description = "Get CPU, disk, or network metrics for a server over a \
        time period. type is repeatable (pass it more than once for more \
        than one metric); start/end are RFC3339 timestamps. Metrics are \
        available for the last 30 days only.",
        annotations(
            title = "Get server metrics",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_server_metrics(
        &self,
        Parameters(args): Parameters<GetServerMetricsArgs>,
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
        if let Some(step) = args.step {
            query.push(("step", step.to_string()));
        }
        respond(
            self.client
                .get(&format!("/servers/{}/metrics", args.id), &query)
                .await,
        )
    }

    #[tool(
        description = "Run a server action, e.g. rebuild, attach_iso, \
        create_image, change_type, reset_password. params carries the \
        per-action request body (e.g. {\"image\": \"ubuntu-24.04\"} for \
        rebuild); omit it for actions with no body. poweron, poweroff, \
        reboot, and shutdown are also available via power_server.",
        annotations(
            title = "Run server action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn server_action(
        &self,
        Parameters(args): Parameters<ServerActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if !SERVER_ACTIONS.contains(&args.action.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "action must be one of {}, got {:?}",
                    SERVER_ACTIONS.join(", "),
                    args.action
                ),
                None,
            ));
        }
        let body = super::action_body(args.params);
        respond(
            self.client
                .post(
                    &format!("/servers/{}/actions/{}", args.id, args.action),
                    body,
                )
                .await,
        )
    }

    #[tool(
        description = "Get one or more actions by ID. The API removed \
        listing all actions, so at least one id is required.",
        annotations(
            title = "List actions",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_actions(
        &self,
        Parameters(args): Parameters<ListActionsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.id.is_empty() {
            return Err(ErrorData::invalid_params(
                "id must contain at least one action ID",
                None,
            ));
        }
        let mut query = Vec::new();
        for id in args.id {
            push_param(&mut query, "id", Some(id.to_string()));
        }
        respond(self.client.get("/actions", &query).await)
    }

    #[tool(
        description = "Get a single action by ID. Actions are asynchronous; \
        poll this until status is success or error.",
        annotations(
            title = "Get action",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_action(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.get(&format!("/actions/{id}"), &[]).await)
    }

    #[tool(
        description = "Get current Hetzner Cloud pricing for all resource types.",
        annotations(
            title = "Get pricing",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_pricing(&self) -> Result<CallToolResult, ErrorData> {
        respond(self.client.get("/pricing", &[]).await)
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::server::test_support::{server_for, tool_result_json};

    #[tokio::test]
    async fn update_server_sends_exactly_the_set_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/servers/1"))
            .and(body_json(serde_json::json!({"name": "web-1"})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"server": {"id": 1}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_server(Parameters(UpdateServerArgs {
                id: 1,
                name: Some("web-1".into()),
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"server": {"id": 1}})
        );
    }

    #[tokio::test]
    async fn update_server_forwards_labels_without_the_id() {
        let mock = MockServer::start().await;
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        Mock::given(method("PUT"))
            .and(path("/servers/2"))
            .and(body_json(serde_json::json!({"labels": {"env": "prod"}})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"server": {"id": 2}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_server(Parameters(UpdateServerArgs {
                id: 2,
                name: None,
                labels: Some(labels),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"server": {"id": 2}})
        );
    }

    #[tokio::test]
    async fn get_server_metrics_passes_the_query_params() {
        // Distinct-id table: if the path were hardcoded to either literal,
        // the *other* call would 404 against the mock and fail this test.
        let mock = MockServer::start().await;
        for id in [5u64, 91] {
            Mock::given(method("GET"))
                .and(path(format!("/servers/{id}/metrics")))
                .and(query_param("type", "cpu"))
                .and(query_param("type", "disk"))
                .and(query_param("start", "2024-01-01T00:00:00Z"))
                .and(query_param("end", "2024-01-02T00:00:00Z"))
                .and(query_param("step", "60"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "metrics": {"for": id}
                })))
                .mount(&mock)
                .await;

            let server = server_for(mock.uri());
            let res = server
                .get_server_metrics(Parameters(GetServerMetricsArgs {
                    id,
                    r#type: vec!["cpu".into(), "disk".into()],
                    start: "2024-01-01T00:00:00Z".into(),
                    end: "2024-01-02T00:00:00Z".into(),
                    step: Some(60.0),
                }))
                .await
                .unwrap();
            assert_eq!(
                tool_result_json(&res),
                serde_json::json!({"metrics": {"for": id}})
            );
        }
    }

    #[tokio::test]
    async fn update_server_rejects_an_all_unset_update_with_invalid_params() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .update_server(Parameters(UpdateServerArgs {
                id: 1,
                name: None,
                labels: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn get_server_metrics_rejects_an_unknown_type_with_invalid_params() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .get_server_metrics(Parameters(GetServerMetricsArgs {
                id: 5,
                r#type: vec!["cpu".into(), "gpu".into()],
                start: "2024-01-01T00:00:00Z".into(),
                end: "2024-01-02T00:00:00Z".into(),
                step: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn get_server_metrics_rejects_an_empty_type_list() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .get_server_metrics(Parameters(GetServerMetricsArgs {
                id: 5,
                r#type: vec![],
                start: "2024-01-01T00:00:00Z".into(),
                end: "2024-01-02T00:00:00Z".into(),
                step: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn server_action_forwards_the_params_body_to_the_action_path() {
        // Distinct-id table: a hardcoded id in the path would 404 on the
        // other iteration's mock and fail this test.
        let mock = MockServer::start().await;
        for id in [7u64, 130] {
            Mock::given(method("POST"))
                .and(path(format!("/servers/{id}/actions/rebuild")))
                .and(body_json(serde_json::json!({"image": "ubuntu-24.04"})))
                .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                    "action": {"id": id}
                })))
                .mount(&mock)
                .await;

            let server = server_for(mock.uri());
            let mut params = serde_json::Map::new();
            params.insert("image".to_string(), serde_json::json!("ubuntu-24.04"));
            let res = server
                .server_action(Parameters(ServerActionArgs {
                    id,
                    action: "rebuild".into(),
                    params: Some(params),
                }))
                .await
                .unwrap();
            assert_eq!(
                tool_result_json(&res),
                serde_json::json!({"action": {"id": id}})
            );
        }
    }

    #[tokio::test]
    async fn server_action_defaults_to_an_empty_body_when_params_is_omitted() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/servers/7/actions/reset"))
            .and(body_json(serde_json::json!({})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .server_action(Parameters(ServerActionArgs {
                id: 7,
                action: "reset".into(),
                params: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn server_action_rejects_unknown_actions_with_invalid_params() {
        // No mock is mounted and nothing listens on the base URL, so a
        // transport-error message would *also* contain "nuke" (it's in the
        // attempted URL) - assert the error kind, not message content.
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .server_action(Parameters(ServerActionArgs {
                id: 7,
                action: "nuke".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn list_actions_sends_repeated_id_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/actions"))
            .and(query_param("id", "1"))
            .and(query_param("id", "2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"actions": [{"id": 1}, {"id": 2}]})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .list_actions(Parameters(ListActionsArgs { id: vec![1, 2] }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"actions": [{"id": 1}, {"id": 2}]})
        );
    }

    #[tokio::test]
    async fn list_actions_rejects_an_empty_id_list_with_invalid_params() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .list_actions(Parameters(ListActionsArgs { id: vec![] }))
            .await
            .unwrap_err();
        assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn get_action_hits_the_id_path() {
        // Distinct-id table: a hardcoded id in the path would 404 on the
        // other iteration's mock and fail this test.
        let mock = MockServer::start().await;
        for id in [42u64, 128] {
            Mock::given(method("GET"))
                .and(path(format!("/actions/{id}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "action": {"id": id, "status": "running"}
                })))
                .mount(&mock)
                .await;

            let server = server_for(mock.uri());
            let res = server.get_action(Parameters(IdArgs { id })).await.unwrap();
            assert_eq!(
                tool_result_json(&res),
                serde_json::json!({"action": {"id": id, "status": "running"}})
            );
        }
    }

    #[tokio::test]
    async fn get_pricing_hits_the_pricing_path() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pricing"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"pricing": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server.get_pricing().await.unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"pricing": {}}));
    }

    /// Pins the allowlist to the spec's 23 POST /servers/{id}/actions/*
    /// operations, in declared order, so an accidental addition/removal/typo
    /// is caught here instead of only surfacing as a runtime 404.
    #[test]
    fn server_actions_is_pinned_to_the_spec_list() {
        assert_eq!(
            SERVER_ACTIONS,
            [
                "add_to_placement_group",
                "attach_iso",
                "attach_to_network",
                "change_alias_ips",
                "change_dns_ptr",
                "change_protection",
                "change_type",
                "create_image",
                "detach_from_network",
                "detach_iso",
                "disable_backup",
                "disable_rescue",
                "enable_backup",
                "enable_rescue",
                "poweroff",
                "poweron",
                "reboot",
                "rebuild",
                "remove_from_placement_group",
                "request_console",
                "reset",
                "reset_password",
                "shutdown",
            ]
        );
    }

    /// N7: pins METRIC_TYPES to its exact expected array (via `.to_vec()` so
    /// an appended/dropped element fails at runtime, not just diverges by
    /// silently changing the const's inferred length).
    #[test]
    fn metric_types_is_pinned_to_the_spec_list() {
        assert_eq!(METRIC_TYPES.to_vec(), vec!["cpu", "disk", "network"]);
    }

    /// Mirrors compute's router annotation assertion: (read_only, destructive)
    /// per tool, so flipping a hint on any of the 6 tools breaks the suite.
    /// Also asserts open_world_hint and distinct, non-empty titles (N6:
    /// parity with net.rs/misc.rs/res_ops.rs).
    #[test]
    fn servers_ops_router_registers_all_6_tools_with_expected_annotations() {
        let router = super::HcloudServer::servers_ops_router();
        let expected: [(&str, bool, bool); 6] = [
            ("update_server", false, false),
            ("get_server_metrics", true, false),
            ("server_action", false, true),
            ("list_actions", true, false),
            ("get_action", true, false),
            ("get_pricing", true, false),
        ];
        assert_eq!(router.list_all().len(), 6);
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
        assert_eq!(titles.len(), 6, "every tool must have a distinct title");
    }
}
