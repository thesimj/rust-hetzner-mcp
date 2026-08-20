//! Infra tools: locations, datacenters, volumes, networks, firewalls.
//! Implemented in milestone M3b (brief B4).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{HcloudServer, map_api_err, ok_json};

/// Numeric ID of the resource to fetch.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct IdArgs {
    /// Numeric ID of the resource.
    pub id: u64,
}

/// Pagination shared by list tools with no label filter.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PageArgs {
    /// Page number to fetch, 1-based.
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    pub per_page: Option<u32>,
}

/// Pagination plus label filter shared by list tools that support it.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct LabelPageArgs {
    /// Label selector to filter results, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Page number to fetch, 1-based.
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    pub per_page: Option<u32>,
}

fn page_query(page: Option<u32>, per_page: Option<u32>) -> Vec<(&'static str, String)> {
    let mut q = Vec::new();
    if let Some(p) = page {
        q.push(("page", p.to_string()));
    }
    if let Some(p) = per_page {
        q.push(("per_page", p.to_string()));
    }
    q
}

fn label_page_query(args: &LabelPageArgs) -> Vec<(&'static str, String)> {
    let mut q = page_query(args.page, args.per_page);
    if let Some(sel) = &args.label_selector {
        q.push(("label_selector", sel.clone()));
    }
    q
}

#[tool_router(router = infra_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "List locations Hetzner Cloud resources can run in.",
        annotations(
            title = "List locations",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_locations(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = page_query(args.page, args.per_page);
        let v = self
            .client
            .get("/locations", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Get a single location by ID.",
        annotations(
            title = "Get location",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_location(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let v = self
            .client
            .get(&format!("/locations/{}", args.id), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List datacenters Hetzner Cloud resources can run in.",
        annotations(
            title = "List datacenters",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_datacenters(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = page_query(args.page, args.per_page);
        let v = self
            .client
            .get("/datacenters", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Get a single datacenter by ID.",
        annotations(
            title = "Get datacenter",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_datacenter(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let v = self
            .client
            .get(&format!("/datacenters/{}", args.id), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List block storage volumes.",
        annotations(
            title = "List volumes",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_volumes(
        &self,
        Parameters(args): Parameters<LabelPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = label_page_query(&args);
        let v = self
            .client
            .get("/volumes", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Get a single volume by ID.",
        annotations(
            title = "Get volume",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_volume(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let v = self
            .client
            .get(&format!("/volumes/{}", args.id), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List private networks.",
        annotations(
            title = "List networks",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_networks(
        &self,
        Parameters(args): Parameters<LabelPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = label_page_query(&args);
        let v = self
            .client
            .get("/networks", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Get a single network by ID.",
        annotations(
            title = "Get network",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_network(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let v = self
            .client
            .get(&format!("/networks/{}", args.id), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "List firewalls.",
        annotations(
            title = "List firewalls",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_firewalls(
        &self,
        Parameters(args): Parameters<LabelPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = label_page_query(&args);
        let v = self
            .client
            .get("/firewalls", &query)
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }

    #[tool(
        description = "Get a single firewall by ID.",
        annotations(
            title = "Get firewall",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_firewall(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let v = self
            .client
            .get(&format!("/firewalls/{}", args.id), &[])
            .await
            .map_err(map_api_err)?;
        ok_json(v)
    }
}

#[cfg(test)]
mod tests {
    use rmcp::handler::server::wrapper::Parameters;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::test_support::{server_for, tool_result_json};
    use super::{IdArgs, LabelPageArgs, PageArgs};

    fn no_page() -> PageArgs {
        PageArgs {
            page: None,
            per_page: None,
        }
    }

    fn no_filter() -> LabelPageArgs {
        LabelPageArgs {
            label_selector: None,
            page: None,
            per_page: None,
        }
    }

    /// Every plain list_* tool (no label filter) passes the envelope through untouched.
    #[tokio::test]
    async fn list_tools_without_a_filter_return_the_envelope() {
        let server = MockServer::start().await;
        for (segment, envelope) in [
            ("locations", serde_json::json!({"locations": [{"id": 1}]})),
            ("datacenters", serde_json::json!({"datacenters": []})),
            ("networks", serde_json::json!({"networks": []})),
            ("firewalls", serde_json::json!({"firewalls": []})),
        ] {
            Mock::given(method("GET"))
                .and(path(format!("/{segment}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
                .mount(&server)
                .await;
        }

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(&hcloud.list_locations(Parameters(no_page())).await.unwrap()),
            serde_json::json!({"locations": [{"id": 1}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_datacenters(Parameters(no_page()))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"datacenters": []})
        );
        assert_eq!(
            tool_result_json(&hcloud.list_networks(Parameters(no_filter())).await.unwrap()),
            serde_json::json!({"networks": []})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_firewalls(Parameters(no_filter()))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"firewalls": []})
        );
    }

    #[tokio::test]
    async fn list_locations_forwards_pagination() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/locations"))
            .and(query_param("page", "2"))
            .and(query_param("per_page", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "locations": []
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .list_locations(Parameters(PageArgs {
                page: Some(2),
                per_page: Some(10),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"locations": []}));
    }

    #[tokio::test]
    async fn list_volumes_forwards_label_selector_and_pagination() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes"))
            .and(query_param("label_selector", "env=prod"))
            .and(query_param("page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"volumes": []})),
            )
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .list_volumes(Parameters(LabelPageArgs {
                label_selector: Some("env=prod".to_string()),
                page: Some(1),
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"volumes": []}));
    }

    /// Every get_* tool builds `/{resource}/{id}` and passes the envelope through.
    #[tokio::test]
    async fn get_tools_hit_their_id_path_and_return_the_envelope() {
        let server = MockServer::start().await;
        for (route, envelope) in [
            ("/locations/42", serde_json::json!({"location": {"id": 42}})),
            (
                "/datacenters/7",
                serde_json::json!({"datacenter": {"id": 7}}),
            ),
            ("/volumes/3", serde_json::json!({"volume": {"id": 3}})),
            ("/networks/5", serde_json::json!({"network": {"id": 5}})),
            ("/firewalls/8", serde_json::json!({"firewall": {"id": 8}})),
        ] {
            Mock::given(method("GET"))
                .and(path(route))
                .respond_with(ResponseTemplate::new(200).set_body_json(&envelope))
                .mount(&server)
                .await;
        }

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_location(Parameters(IdArgs { id: 42 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"location": {"id": 42}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_datacenter(Parameters(IdArgs { id: 7 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"datacenter": {"id": 7}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_volume(Parameters(IdArgs { id: 3 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"volume": {"id": 3}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_network(Parameters(IdArgs { id: 5 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"network": {"id": 5}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_firewall(Parameters(IdArgs { id: 8 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"firewall": {"id": 8}})
        );
    }

    #[tokio::test]
    async fn get_location_maps_upstream_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/locations/9"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": "not_found", "message": "location not found"}
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let err = hcloud
            .get_location(Parameters(IdArgs { id: 9 }))
            .await
            .unwrap_err();
        assert!(err.message.contains("not_found"), "got: {}", err.message);
    }

    #[test]
    fn infra_router_registers_all_ten_tools() {
        let router = super::HcloudServer::infra_router();
        for name in [
            "list_locations",
            "get_location",
            "list_datacenters",
            "get_datacenter",
            "list_volumes",
            "get_volume",
            "list_networks",
            "get_network",
            "list_firewalls",
            "get_firewall",
        ] {
            assert!(router.has_route(name), "missing route: {name}");
        }
        assert_eq!(router.list_all().len(), 10);
    }
}
