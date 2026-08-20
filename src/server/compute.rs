//! Compute tools: servers, images, server_types, ssh_keys. Implemented in
//! milestone M3a (brief B3).

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData, tool, tool_router};
use serde::Deserialize;
use serde_json::Value;

use super::{HcloudServer, map_api_err, ok_json};

const POWER_ACTIONS: [&str; 4] = ["poweron", "poweroff", "reboot", "shutdown"];

/// Numeric ID of a single resource, shared by the get/delete-by-id tools.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct IdArgs {
    /// Numeric ID of the resource.
    pub id: u64,
}

/// `page`/`per_page` pagination, shared by the list tools with no other filters.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PageArgs {
    /// Page number, 1-based.
    pub page: Option<u32>,
    /// Results per page (default 25, max 50).
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListServersArgs {
    /// Exact server name to filter by.
    pub name: Option<String>,
    /// Label selector, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Filter by status; repeatable (initializing|starting|running|stopping|off|deleting|rebuilding|migrating|unknown).
    pub status: Option<Vec<String>>,
    /// Sort order, e.g. "id:asc" or "name:desc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Page number, 1-based.
    pub page: Option<u32>,
    /// Results per page (default 25, max 50).
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateServerArgs {
    /// Name for the new server.
    pub name: String,
    /// Server type name or ID, e.g. "cx22".
    pub server_type: String,
    /// Image name or ID to boot from.
    pub image: String,
    /// Location name to create the server in.
    pub location: Option<String>,
    /// Datacenter name to create the server in.
    pub datacenter: Option<String>,
    /// SSH key names or IDs to install on the server.
    pub ssh_keys: Option<Vec<String>>,
    /// Cloud-init user data.
    pub user_data: Option<String>,
    /// Labels to attach to the server.
    pub labels: Option<BTreeMap<String, String>>,
    /// Whether to start the server right after creation.
    pub start_after_create: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PowerServerArgs {
    /// Server ID to act on.
    pub id: u64,
    /// Power action: one of poweron, poweroff, reboot, shutdown.
    pub action: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListImagesArgs {
    /// Filter by image type, e.g. "system" or "snapshot".
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    /// Page number, 1-based.
    pub page: Option<u32>,
    /// Results per page (default 25, max 50).
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateSshKeyArgs {
    /// Name for the new SSH key.
    pub name: String,
    /// Public key material (OpenSSH format).
    pub public_key: String,
    /// Labels to attach to the SSH key.
    pub labels: Option<BTreeMap<String, String>>,
}

/// `page`/`per_page` as query params, omitting unset ones.
fn pagination_query(page: Option<u32>, per_page: Option<u32>) -> Vec<(&'static str, String)> {
    let mut query = Vec::new();
    if let Some(page) = page {
        query.push(("page", page.to_string()));
    }
    if let Some(per_page) = per_page {
        query.push(("per_page", per_page.to_string()));
    }
    query
}

#[tool_router(router = compute_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "List servers, optionally filtered by name, label selector, or status.",
        annotations(title = "List servers", read_only_hint = true, open_world_hint = true)
    )]
    pub(crate) async fn list_servers(
        &self,
        Parameters(args): Parameters<ListServersArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page);
        if let Some(name) = args.name {
            query.push(("name", name));
        }
        if let Some(label_selector) = args.label_selector {
            query.push(("label_selector", label_selector));
        }
        for status in args.status.into_iter().flatten() {
            query.push(("status", status));
        }
        for sort in args.sort.into_iter().flatten() {
            query.push(("sort", sort));
        }
        let value = self
            .client
            .get("/servers", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Get a single server by ID.",
        annotations(title = "Get server", read_only_hint = true, open_world_hint = true)
    )]
    pub(crate) async fn get_server(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let value = self
            .client
            .get(&format!("/servers/{id}"), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Create a new server. This creates a BILLABLE resource. The response \
        contains the root_password exactly once - it is not retrievable afterwards.",
        annotations(
            title = "Create server",
            read_only_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_server(
        &self,
        Parameters(args): Parameters<CreateServerArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut body = serde_json::Map::new();
        body.insert("name".to_string(), Value::String(args.name));
        body.insert("server_type".to_string(), Value::String(args.server_type));
        body.insert("image".to_string(), Value::String(args.image));
        if let Some(location) = args.location {
            body.insert("location".to_string(), Value::String(location));
        }
        if let Some(datacenter) = args.datacenter {
            body.insert("datacenter".to_string(), Value::String(datacenter));
        }
        if let Some(ssh_keys) = args.ssh_keys {
            body.insert("ssh_keys".to_string(), Value::from(ssh_keys));
        }
        if let Some(user_data) = args.user_data {
            body.insert("user_data".to_string(), Value::String(user_data));
        }
        if let Some(labels) = args.labels {
            body.insert(
                "labels".to_string(),
                serde_json::to_value(labels).expect("BTreeMap<String,String> always serializes"),
            );
        }
        if let Some(start_after_create) = args.start_after_create {
            body.insert(
                "start_after_create".to_string(),
                Value::Bool(start_after_create),
            );
        }
        let value = self
            .client
            .post("/servers", Value::Object(body))
            .await
            .map_err(map_api_err)?;
        ok_json(value)
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
        let value = self
            .client
            .delete(&format!("/servers/{id}"))
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Run a power action (poweron, poweroff, reboot, shutdown) on a server. \
        poweroff and shutdown stop workloads running on the server.",
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
        let value = self
            .client
            .post(
                &format!("/servers/{}/actions/{}", args.id, args.action),
                serde_json::json!({}),
            )
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "List available images, optionally filtered by type.",
        annotations(title = "List images", read_only_hint = true, open_world_hint = true)
    )]
    pub(crate) async fn list_images(
        &self,
        Parameters(args): Parameters<ListImagesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page);
        if let Some(r#type) = args.r#type {
            query.push(("type", r#type));
        }
        let value = self
            .client
            .get("/images", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Get a single image by ID.",
        annotations(title = "Get image", read_only_hint = true, open_world_hint = true)
    )]
    pub(crate) async fn get_image(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let value = self
            .client
            .get(&format!("/images/{id}"), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "List available server types (plans).",
        annotations(
            title = "List server types",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn list_server_types(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = pagination_query(args.page, args.per_page);
        let value = self
            .client
            .get("/server_types", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Get a single server type by ID.",
        annotations(
            title = "Get server type",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn get_server_type(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let value = self
            .client
            .get(&format!("/server_types/{id}"), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "List SSH keys uploaded to the project.",
        annotations(title = "List SSH keys", read_only_hint = true, open_world_hint = true)
    )]
    pub(crate) async fn list_ssh_keys(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = pagination_query(args.page, args.per_page);
        let value = self
            .client
            .get("/ssh_keys", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Get a single SSH key by ID.",
        annotations(title = "Get SSH key", read_only_hint = true, open_world_hint = true)
    )]
    pub(crate) async fn get_ssh_key(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let value = self
            .client
            .get(&format!("/ssh_keys/{id}"), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Upload a new SSH key to the project.",
        annotations(
            title = "Create SSH key",
            read_only_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_ssh_key(
        &self,
        Parameters(args): Parameters<CreateSshKeyArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut body = serde_json::Map::new();
        body.insert("name".to_string(), Value::String(args.name));
        body.insert("public_key".to_string(), Value::String(args.public_key));
        if let Some(labels) = args.labels {
            body.insert(
                "labels".to_string(),
                serde_json::to_value(labels).expect("BTreeMap<String,String> always serializes"),
            );
        }
        let value = self
            .client
            .post("/ssh_keys", Value::Object(body))
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }

    #[tool(
        description = "Delete an SSH key from the project.",
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
        let value = self
            .client
            .delete(&format!("/ssh_keys/{id}"))
            .await
            .map_err(map_api_err)?;
        ok_json(value)
    }
}

#[cfg(test)]
mod tests {
    use rmcp::handler::server::wrapper::Parameters;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::server::test_support::{server_for, tool_result_json};

    #[tokio::test]
    async fn list_servers_passes_filters_as_repeated_query_params() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .and(query_param("name", "web-1"))
            .and(query_param("status", "running"))
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
                label_selector: None,
                status: Some(vec!["running".into()]),
                sort: None,
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
    async fn list_servers_maps_upstream_errors() {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/servers"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let err = server
            .list_servers(Parameters(ListServersArgs {
                name: None,
                label_selector: None,
                status: None,
                sort: None,
                page: None,
                per_page: None,
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("500"), "got: {}", err.message);
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
    async fn create_server_omits_unset_optional_fields() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/servers"))
            .and(body_partial_json(serde_json::json!({
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
        Mock::given(method("POST"))
            .and(path("/servers"))
            .and(body_partial_json(serde_json::json!({
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
        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        server
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
        server
            .delete_server(Parameters(IdArgs { id: 7 }))
            .await
            .unwrap();
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
        server
            .power_server(Parameters(PowerServerArgs {
                id: 7,
                action: "reboot".into(),
            }))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn power_server_rejects_unknown_actions() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .power_server(Parameters(PowerServerArgs {
                id: 7,
                action: "nuke".into(),
            }))
            .await
            .unwrap_err();
        assert!(err.message.contains("nuke"), "got: {}", err.message);
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
        server
            .get_image(Parameters(IdArgs { id: 5 }))
            .await
            .unwrap();
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
        server
            .list_server_types(Parameters(PageArgs {
                page: Some(2),
                per_page: Some(10),
            }))
            .await
            .unwrap();
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
        server
            .get_server_type(Parameters(IdArgs { id: 3 }))
            .await
            .unwrap();
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
        server
            .list_ssh_keys(Parameters(PageArgs {
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
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
        server
            .get_ssh_key(Parameters(IdArgs { id: 9 }))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_ssh_key_posts_the_body() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ssh_keys"))
            .and(body_partial_json(serde_json::json!({
                "name": "laptop",
                "public_key": "ssh-ed25519 AAAA..."
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "ssh_key": {"id": 10}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        server
            .create_ssh_key(Parameters(CreateSshKeyArgs {
                name: "laptop".into(),
                public_key: "ssh-ed25519 AAAA...".into(),
                labels: None,
            }))
            .await
            .unwrap();
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
        server
            .delete_ssh_key(Parameters(IdArgs { id: 10 }))
            .await
            .unwrap();
    }
}
