//! Infra tools: locations, datacenters, volumes, networks, firewalls.
//! Implemented in milestone M3b (brief B4).
//!
//! Args structs are shared across tools with the same shape (`IdArgs` for
//! every get_*, `PageArgs`/`LabelPageArgs` for list_*) rather than one struct
//! per tool - rmcp strips struct-level doc comments from the schema it sends
//! the client, so only field docs are wire-visible and the sharing costs
//! nothing there.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{HcloudServer, IdArgs, PageArgs, pagination_query, push_param, respond};

/// Pagination plus name/label/sort filters, shared by list_networks and
/// list_firewalls (identical filter sets per spec). list_volumes adds
/// `status` on top of this shape, so it gets its own struct.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LabelPageArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Label selector to filter results, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Sort order, e.g. "id:asc" or "name:desc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

fn label_page_query(args: LabelPageArgs) -> Result<Vec<(&'static str, String)>, ErrorData> {
    let mut q = pagination_query(args.page, args.per_page)?;
    push_param(&mut q, "name", args.name);
    push_param(&mut q, "label_selector", args.label_selector);
    for sort in args.sort.into_iter().flatten() {
        push_param(&mut q, "sort", Some(sort));
    }
    Ok(q)
}

/// Filters for list_volumes: name/label/sort plus a `status` filter the
/// other two label_page_query tools don't support.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListVolumesArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Label selector to filter results, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Sort order, e.g. "id:asc" or "name:desc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Filter by status, e.g. "available" or "creating"; repeatable.
    pub status: Option<Vec<String>>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
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
        let query = pagination_query(args.page, args.per_page)?;
        respond(self.client.get("/locations", &query).await)
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
        respond(
            self.client
                .get(&format!("/locations/{}", args.id), &[])
                .await,
        )
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
        let query = pagination_query(args.page, args.per_page)?;
        respond(self.client.get("/datacenters", &query).await)
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
        respond(
            self.client
                .get(&format!("/datacenters/{}", args.id), &[])
                .await,
        )
    }

    #[tool(
        description = "List block storage volumes, optionally filtered by name, label \
        selector, sort, or status.",
        annotations(
            title = "List volumes",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_volumes(
        &self,
        Parameters(args): Parameters<ListVolumesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page)?;
        push_param(&mut query, "name", args.name);
        push_param(&mut query, "label_selector", args.label_selector);
        for sort in args.sort.into_iter().flatten() {
            push_param(&mut query, "sort", Some(sort));
        }
        for status in args.status.into_iter().flatten() {
            push_param(&mut query, "status", Some(status));
        }
        respond(self.client.get("/volumes", &query).await)
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
        respond(self.client.get(&format!("/volumes/{}", args.id), &[]).await)
    }

    #[tool(
        description = "List private networks, optionally filtered by name, label selector, \
        or sort.",
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
        let query = label_page_query(args)?;
        respond(self.client.get("/networks", &query).await)
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
        respond(
            self.client
                .get(&format!("/networks/{}", args.id), &[])
                .await,
        )
    }

    #[tool(
        description = "List firewalls, optionally filtered by name, label selector, or sort.",
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
        let query = label_page_query(args)?;
        respond(self.client.get("/firewalls", &query).await)
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
        respond(
            self.client
                .get(&format!("/firewalls/{}", args.id), &[])
                .await,
        )
    }
}

#[cfg(test)]
mod tests {
    use rmcp::handler::server::wrapper::Parameters;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::super::test_support::{server_for, tool_result_json};
    use super::{IdArgs, LabelPageArgs, ListVolumesArgs, PageArgs};

    /// Every list_* tool forwards page/per_page (and label_selector, where the
    /// tool takes one) verbatim, and passes the response envelope through.
    #[tokio::test]
    async fn list_tools_forward_pagination_and_label_and_return_the_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/locations"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "25"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "locations": [{"id": 1}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/datacenters"))
            .and(query_param("page", "2"))
            .and(query_param("per_page", "30"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "datacenters": [{"id": 2}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/volumes"))
            .and(query_param("name", "data-1"))
            .and(query_param("label_selector", "env=prod"))
            .and(query_param("sort", "id:asc"))
            .and(query_param("status", "available"))
            .and(query_param("page", "3"))
            .and(query_param("per_page", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "volumes": [{"id": 3}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/networks"))
            .and(query_param("name", "net-1"))
            .and(query_param("label_selector", "team=x"))
            .and(query_param("sort", "name:asc"))
            .and(query_param("page", "4"))
            .and(query_param("per_page", "15"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "networks": [{"id": 4}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/firewalls"))
            .and(query_param("name", "fw-1"))
            .and(query_param("label_selector", "app=y"))
            .and(query_param("sort", "name:desc"))
            .and(query_param("page", "5"))
            .and(query_param("per_page", "20"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "firewalls": [{"id": 5}]
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_locations(Parameters(PageArgs {
                        page: Some(1),
                        per_page: Some(25)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"locations": [{"id": 1}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_datacenters(Parameters(PageArgs {
                        page: Some(2),
                        per_page: Some(30)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"datacenters": [{"id": 2}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_volumes(Parameters(ListVolumesArgs {
                        name: Some("data-1".to_string()),
                        label_selector: Some("env=prod".to_string()),
                        sort: Some(vec!["id:asc".to_string()]),
                        status: Some(vec!["available".to_string()]),
                        page: Some(3),
                        per_page: Some(10)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"volumes": [{"id": 3}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_networks(Parameters(LabelPageArgs {
                        name: Some("net-1".to_string()),
                        label_selector: Some("team=x".to_string()),
                        sort: Some(vec!["name:asc".to_string()]),
                        page: Some(4),
                        per_page: Some(15)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"networks": [{"id": 4}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_firewalls(Parameters(LabelPageArgs {
                        name: Some("fw-1".to_string()),
                        label_selector: Some("app=y".to_string()),
                        sort: Some(vec!["name:desc".to_string()]),
                        page: Some(5),
                        per_page: Some(20)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"firewalls": [{"id": 5}]})
        );
    }

    /// F1: an empty label_selector must be dropped, not sent as `?label_selector=`
    /// (Hetzner 400s on that). The mock only matches a request with the param absent.
    #[tokio::test]
    async fn list_volumes_drops_an_empty_label_selector() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/volumes"))
            .and(query_param_is_missing("label_selector"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"volumes": []})),
            )
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .list_volumes(Parameters(ListVolumesArgs {
                name: None,
                label_selector: Some(String::new()),
                sort: None,
                status: None,
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"volumes": []}));
    }

    /// Every get_* tool builds `/{resource}/{id}` and passes the envelope through.
    /// F2: also proves the id is actually interpolated, not hardcoded - a
    /// request for an unmounted id must fail rather than silently succeed.
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

        let unmounted = hcloud
            .get_network(Parameters(IdArgs { id: 999 }))
            .await
            .unwrap();
        assert_eq!(
            unmounted.is_error,
            Some(true),
            "an id with no mounted mock must not resolve to another tool's route"
        );
    }

    #[tokio::test]
    async fn get_location_maps_upstream_errors_to_a_tool_error_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/locations/9"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": "not_found", "message": "location not found"}
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .get_location(Parameters(IdArgs { id: 9 }))
            .await
            .unwrap();
        assert_eq!(res.is_error, Some(true));
        let v = serde_json::to_value(&res).unwrap();
        let text = v["content"][0]["text"].as_str().expect("text content");
        assert!(text.contains("not_found"), "got: {text}");
    }

    #[test]
    fn infra_router_registers_all_ten_tools_with_read_only_annotations() {
        let router = super::HcloudServer::infra_router();
        let names = [
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
        ];
        assert_eq!(router.list_all().len(), 10);
        for name in names {
            let tool = router
                .get(name)
                .unwrap_or_else(|| panic!("missing route: {name}"));
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{name} has no annotations"));
            assert_eq!(
                annotations.read_only_hint,
                Some(true),
                "{name} must be read_only_hint = true"
            );
            assert_eq!(
                annotations.destructive_hint,
                Some(false),
                "{name} must be destructive_hint = false"
            );
        }
    }
}
