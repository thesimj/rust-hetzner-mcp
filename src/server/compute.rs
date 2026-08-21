//! Compute tools: servers, images, server_types, ssh_keys. Implemented in
//! milestone M3a (brief B3), fixed per review in B10.
//!
//! `IdArgs` and `PageArgs` are shared across several tools instead of a
//! bespoke struct per tool - the id/pagination shape, docs, and validation
//! are identical, and a per-tool struct would only rename the field's doc
//! comment. This is a deliberate schema tradeoff (a slightly more generic
//! per-field description), not an oversight; a new id- or pagination-only
//! tool added here does not need its own struct.

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{HcloudServer, IdArgs, pagination_query, push_param, respond};

const POWER_ACTIONS: [&str; 4] = ["poweron", "poweroff", "reboot", "shutdown"];

/// `page`/`per_page` pagination, shared by the list tools with no other filters.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PageArgs {
    /// Page number, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page (default 25, max 50).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListServersArgs {
    /// Exact server name to filter by.
    pub name: Option<String>,
    /// Label selector, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Filter by status; repeatable (initializing|starting|running|stopping|off|deleting|rebuilding|migrating|unknown).
    pub status: Option<Vec<String>>,
    /// Sort order, e.g. "id:asc" or "name:desc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Page number, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page (default 25, max 50).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateServerArgs {
    /// Name for the new server.
    pub name: String,
    /// Server type name or ID, e.g. "cx22".
    pub server_type: String,
    /// Image name or ID to boot from.
    pub image: String,
    /// Location name to create the server in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Datacenter name to create the server in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub datacenter: Option<String>,
    /// SSH key names or IDs to install on the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_keys: Option<Vec<String>>,
    /// Cloud-init user data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<String>,
    /// Labels to attach to the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
    /// Whether to start the server right after creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_after_create: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PowerServerArgs {
    /// Server ID to act on.
    pub id: u64,
    /// Power action: one of poweron, poweroff, reboot, shutdown.
    pub action: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListImagesArgs {
    /// Filter by image type, e.g. "system" or "snapshot".
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    /// Page number, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page (default 25, max 50).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateSshKeyArgs {
    /// Name for the new SSH key.
    pub name: String,
    /// Public key material (OpenSSH format).
    pub public_key: String,
    /// Labels to attach to the SSH key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[tool_router(router = compute_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "List servers, optionally filtered by name, label selector, or status.",
        annotations(
            title = "List servers",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_servers(
        &self,
        Parameters(args): Parameters<ListServersArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page);
        push_param(&mut query, "name", args.name);
        push_param(&mut query, "label_selector", args.label_selector);
        for status in args.status.into_iter().flatten() {
            push_param(&mut query, "status", Some(status));
        }
        for sort in args.sort.into_iter().flatten() {
            push_param(&mut query, "sort", Some(sort));
        }
        respond(self.client.get("/servers", &query).await)
    }

    #[tool(
        description = "Get a single server by ID.",
        annotations(
            title = "Get server",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_server(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.get(&format!("/servers/{id}"), &[]).await)
    }

    #[tool(
        description = "Create a new server. This creates a BILLABLE resource. The response \
        contains the root_password exactly once - it is not retrievable afterwards.",
        annotations(
            title = "Create server",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_server(
        &self,
        Parameters(args): Parameters<CreateServerArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/servers", body).await)
    }

    #[tool(
        description = "Delete a server permanently. This destroys the server and all its data.",
        annotations(
            title = "Delete server",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_server(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/servers/{id}")).await)
    }

    #[tool(
        description = "Run a power action (poweron, poweroff, reboot, shutdown) on a server. \
        poweroff, reboot, and shutdown all interrupt workloads running on the server. For \
        rebuild, change_type, create_image, and other actions, use server_action.",
        annotations(
            title = "Power server",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn power_server(
        &self,
        Parameters(args): Parameters<PowerServerArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if !POWER_ACTIONS.contains(&args.action.as_str()) {
            return Err(ErrorData::invalid_params(
                format!(
                    "action must be one of {}, got {:?}",
                    POWER_ACTIONS.join(", "),
                    args.action
                ),
                None,
            ));
        }
        respond(
            self.client
                .post(
                    &format!("/servers/{}/actions/{}", args.id, args.action),
                    serde_json::json!({}),
                )
                .await,
        )
    }

    #[tool(
        description = "List available images, optionally filtered by type.",
        annotations(
            title = "List images",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_images(
        &self,
        Parameters(args): Parameters<ListImagesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page);
        push_param(&mut query, "type", args.r#type);
        respond(self.client.get("/images", &query).await)
    }

    #[tool(
        description = "Get a single image by ID.",
        annotations(
            title = "Get image",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_image(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.get(&format!("/images/{id}"), &[]).await)
    }

    #[tool(
        description = "List available server types (plans).",
        annotations(
            title = "List server types",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_server_types(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = pagination_query(args.page, args.per_page);
        respond(self.client.get("/server_types", &query).await)
    }

    #[tool(
        description = "Get a single server type by ID.",
        annotations(
            title = "Get server type",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_server_type(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.get(&format!("/server_types/{id}"), &[]).await)
    }

    #[tool(
        description = "List SSH keys uploaded to the project.",
        annotations(
            title = "List SSH keys",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_ssh_keys(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = pagination_query(args.page, args.per_page);
        respond(self.client.get("/ssh_keys", &query).await)
    }

    #[tool(
        description = "Get a single SSH key by ID.",
        annotations(
            title = "Get SSH key",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_ssh_key(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.get(&format!("/ssh_keys/{id}"), &[]).await)
    }

    #[tool(
        description = "Upload a new SSH key to the project.",
        annotations(
            title = "Create SSH key",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_ssh_key(
        &self,
        Parameters(args): Parameters<CreateSshKeyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/ssh_keys", body).await)
    }

    #[tool(
        description = "Delete an SSH key from the project permanently.",
        annotations(
            title = "Delete SSH key",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_ssh_key(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/ssh_keys/{id}")).await)
    }
}

#[cfg(test)]
mod tests {
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::ErrorCode;
    use wiremock::matchers::{body_json, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::server::test_support::{server_for, tool_result_json};

    /// Extract the plain text of an `isError` tool result (not JSON, so
    /// `tool_result_json` doesn't apply).
    fn error_text(res: &CallToolResult) -> String {
        let v = serde_json::to_value(res).unwrap();
        v["content"][0]["text"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn list_servers_passes_filters_as_repeated_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .and(query_param("name", "web-1"))
            .and(query_param("status", "running"))
            .and(query_param("status", "off"))
            .and(query_param("sort", "id:asc"))
            .and(query_param("sort", "name:desc"))
            .and(query_param_is_missing("label_selector"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"servers": [{"id": 1}]})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .list_servers(Parameters(ListServersArgs {
                name: Some("web-1".into()),
                label_selector: Some(String::new()),
                status: Some(vec!["running".into(), "off".into()]),
                sort: Some(vec!["id:asc".into(), "name:desc".into()]),
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"servers": [{"id": 1}]})
        );
    }

    #[tokio::test]
    async fn list_servers_maps_upstream_errors_to_an_error_result() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .list_servers(Parameters(ListServersArgs {
                name: None,
                label_selector: None,
                status: None,
                sort: None,
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(res.is_error, Some(true));
        let text = error_text(&res);
        assert!(text.contains("500"), "got: {text}");
    }

    #[tokio::test]
    async fn get_server_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers/42"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"server": {"id": 42}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .get_server(Parameters(IdArgs { id: 42 }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"server": {"id": 42}})
        );
    }

    #[tokio::test]
    async fn create_server_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/servers"))
            .and(body_json(serde_json::json!({
                "name": "web-1",
                "server_type": "cx22",
                "image": "ubuntu-24.04"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "server": {"id": 1}, "root_password": "s3cret", "action": {}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_server(Parameters(CreateServerArgs {
                name: "web-1".into(),
                server_type: "cx22".into(),
                image: "ubuntu-24.04".into(),
                location: None,
                datacenter: None,
                ssh_keys: None,
                user_data: None,
                labels: None,
                start_after_create: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res)["root_password"],
            serde_json::json!("s3cret")
        );
    }

    #[tokio::test]
    async fn create_server_forwards_optional_fields() {
        let mock = MockServer::start().await;
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        Mock::given(method("POST"))
            .and(path("/servers"))
            .and(body_json(serde_json::json!({
                "name": "web-2",
                "server_type": "cx22",
                "image": "ubuntu-24.04",
                "location": "fsn1",
                "ssh_keys": ["my-key"],
                "labels": {"env": "prod"},
                "start_after_create": false
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"server": {"id": 2}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_server(Parameters(CreateServerArgs {
                name: "web-2".into(),
                server_type: "cx22".into(),
                image: "ubuntu-24.04".into(),
                location: Some("fsn1".into()),
                datacenter: None,
                ssh_keys: Some(vec!["my-key".into()]),
                user_data: None,
                labels: Some(labels),
                start_after_create: Some(false),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"server": {"id": 2}})
        );
    }

    #[tokio::test]
    async fn delete_server_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/servers/7"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_server(Parameters(IdArgs { id: 7 }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn power_server_posts_the_action_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/servers/7/actions/reboot"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .power_server(Parameters(PowerServerArgs {
                id: 7,
                action: "reboot".into(),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn power_server_rejects_unknown_actions_with_invalid_params() {
        // No mock is mounted, and the base URL has nothing listening on it,
        // so a transport-error message would *also* contain "nuke" (it's in
        // the attempted URL) - assert the error kind, not message content.
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .power_server(Parameters(PowerServerArgs {
                id: 7,
                action: "nuke".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn list_images_passes_type_filter() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/images"))
            .and(query_param("type", "snapshot"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"images": []})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .list_images(Parameters(ListImagesArgs {
                r#type: Some("snapshot".into()),
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"images": []}));
    }

    #[tokio::test]
    async fn get_image_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/images/5"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"image": {"id": 5}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .get_image(Parameters(IdArgs { id: 5 }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"image": {"id": 5}})
        );
    }

    #[tokio::test]
    async fn list_server_types_paginates() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/server_types"))
            .and(query_param("page", "2"))
            .and(query_param("per_page", "10"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"server_types": []})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .list_server_types(Parameters(PageArgs {
                page: Some(2),
                per_page: Some(10),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"server_types": []})
        );
    }

    #[tokio::test]
    async fn get_server_type_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/server_types/3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "server_type": {"id": 3}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .get_server_type(Parameters(IdArgs { id: 3 }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"server_type": {"id": 3}})
        );
    }

    #[tokio::test]
    async fn list_ssh_keys_returns_the_envelope() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ssh_keys"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"ssh_keys": []})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .list_ssh_keys(Parameters(PageArgs {
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"ssh_keys": []}));
    }

    #[tokio::test]
    async fn get_ssh_key_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ssh_keys/9"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ssh_key": {"id": 9}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .get_ssh_key(Parameters(IdArgs { id: 9 }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"ssh_key": {"id": 9}})
        );
    }

    #[tokio::test]
    async fn create_ssh_key_posts_the_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ssh_keys"))
            .and(body_json(serde_json::json!({
                "name": "laptop",
                "public_key": "ssh-ed25519 AAAA..."
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "ssh_key": {"id": 10}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_ssh_key(Parameters(CreateSshKeyArgs {
                name: "laptop".into(),
                public_key: "ssh-ed25519 AAAA...".into(),
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"ssh_key": {"id": 10}})
        );
    }

    #[tokio::test]
    async fn delete_ssh_key_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/ssh_keys/10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_ssh_key(Parameters(IdArgs { id: 10 }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({}));
    }

    /// Mirrors infra's router annotation assertion: (read_only, destructive)
    /// per tool, so flipping a hint on any of the 13 tools breaks the suite.
    #[test]
    fn compute_router_registers_all_13_tools_with_expected_annotations() {
        let router = super::HcloudServer::compute_router();
        let expected: [(&str, bool, bool); 13] = [
            ("list_servers", true, false),
            ("get_server", true, false),
            ("create_server", false, false),
            ("delete_server", false, true),
            ("power_server", false, true),
            ("list_images", true, false),
            ("get_image", true, false),
            ("list_server_types", true, false),
            ("get_server_type", true, false),
            ("list_ssh_keys", true, false),
            ("get_ssh_key", true, false),
            ("create_ssh_key", false, false),
            ("delete_ssh_key", false, true),
        ];
        assert_eq!(router.list_all().len(), 13);
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
