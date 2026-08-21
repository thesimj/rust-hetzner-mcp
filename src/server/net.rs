//! Network-adjacent tools: floating_ips, primary_ips, load_balancers,
//! load_balancer_types. Implemented in milestone M9a (brief B19).
//!
//! Filter sets differ per endpoint (verified against the Hetzner spec), so
//! args structs are shared only where the shape is genuinely identical:
//! `NameLabelSortPageArgs` covers both list_floating_ips and
//! list_load_balancers, which take the same name/label_selector/sort/page
//! fields - though the `sort` value *enum* differs (load_balancers also
//! accepts name:asc/name:desc; floating_ips does not). list_primary_ips adds
//! an `ip` filter and list_load_balancer_types drops label_selector/sort, so
//! each gets its own struct.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{HcloudServer, IdArgs, pagination_query, push_param, respond};

/// Pagination plus name/label/sort filters, shared by list_floating_ips and
/// list_load_balancers.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NameLabelSortPageArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Label selector to filter results, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Sort order, e.g. "id:asc" or "created:desc"; repeatable. (Only
    /// list_load_balancers also accepts name:asc/name:desc.)
    pub sort: Option<Vec<String>>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

fn name_label_sort_query(args: NameLabelSortPageArgs) -> Vec<(&'static str, String)> {
    let mut q = pagination_query(args.page, args.per_page);
    push_param(&mut q, "name", args.name);
    push_param(&mut q, "label_selector", args.label_selector);
    for sort in args.sort.into_iter().flatten() {
        push_param(&mut q, "sort", Some(sort));
    }
    q
}

/// Filters for list_primary_ips: name, label selector, exact IP, sort, and pagination.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListPrimaryIpsArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Label selector to filter results, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Exact IP address to filter by.
    pub ip: Option<String>,
    /// Sort order, e.g. "id:asc" or "created:desc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

/// Filters for list_load_balancer_types: name and pagination only - this
/// endpoint has no label_selector or sort parameter.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListLoadBalancerTypesArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

#[tool_router(router = net_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "List Floating IPs, optionally filtered by name, label selector, or sort order.",
        annotations(
            title = "List floating IPs",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_floating_ips(
        &self,
        Parameters(args): Parameters<NameLabelSortPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = name_label_sort_query(args);
        respond(self.client.get("/floating_ips", &query).await)
    }

    #[tool(
        description = "Get a single Floating IP by ID.",
        annotations(
            title = "Get floating IP",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_floating_ip(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.client
                .get(&format!("/floating_ips/{}", args.id), &[])
                .await,
        )
    }

    #[tool(
        description = "List Primary IPs, optionally filtered by name, label selector, IP, or sort order.",
        annotations(
            title = "List primary IPs",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_primary_ips(
        &self,
        Parameters(args): Parameters<ListPrimaryIpsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page);
        push_param(&mut query, "name", args.name);
        push_param(&mut query, "label_selector", args.label_selector);
        push_param(&mut query, "ip", args.ip);
        for sort in args.sort.into_iter().flatten() {
            push_param(&mut query, "sort", Some(sort));
        }
        respond(self.client.get("/primary_ips", &query).await)
    }

    #[tool(
        description = "Get a single Primary IP by ID.",
        annotations(
            title = "Get primary IP",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_primary_ip(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.client
                .get(&format!("/primary_ips/{}", args.id), &[])
                .await,
        )
    }

    #[tool(
        description = "List Load Balancers, optionally filtered by name, label selector, or sort order.",
        annotations(
            title = "List load balancers",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_load_balancers(
        &self,
        Parameters(args): Parameters<NameLabelSortPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = name_label_sort_query(args);
        respond(self.client.get("/load_balancers", &query).await)
    }

    #[tool(
        description = "Get a single Load Balancer by ID.",
        annotations(
            title = "Get load balancer",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_load_balancer(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.client
                .get(&format!("/load_balancers/{}", args.id), &[])
                .await,
        )
    }

    #[tool(
        description = "List Load Balancer types, optionally filtered by name.",
        annotations(
            title = "List load balancer types",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_load_balancer_types(
        &self,
        Parameters(args): Parameters<ListLoadBalancerTypesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page);
        push_param(&mut query, "name", args.name);
        respond(self.client.get("/load_balancer_types", &query).await)
    }

    #[tool(
        description = "Get a single Load Balancer type by ID.",
        annotations(
            title = "Get load balancer type",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_load_balancer_type(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.client
                .get(&format!("/load_balancer_types/{}", args.id), &[])
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
    use super::{IdArgs, ListLoadBalancerTypesArgs, ListPrimaryIpsArgs, NameLabelSortPageArgs};

    /// Every list_* tool forwards page/per_page and its declared filters
    /// verbatim, and passes the response envelope through.
    #[tokio::test]
    async fn list_tools_forward_pagination_and_filters_and_return_the_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/floating_ips"))
            .and(query_param("name", "web-1"))
            .and(query_param("label_selector", "env=prod"))
            .and(query_param("sort", "id:asc"))
            .and(query_param("sort", "created:desc"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "25"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "floating_ips": [{"id": 1}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/primary_ips"))
            .and(query_param("name", "web-2"))
            .and(query_param("label_selector", "env=dev"))
            .and(query_param("ip", "1.2.3.4"))
            .and(query_param("sort", "created:desc"))
            .and(query_param("page", "2"))
            .and(query_param("per_page", "30"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "primary_ips": [{"id": 2}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/load_balancers"))
            .and(query_param("name", "lb-1"))
            .and(query_param("label_selector", "team=x"))
            .and(query_param("sort", "name:desc"))
            .and(query_param("page", "3"))
            .and(query_param("per_page", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "load_balancers": [{"id": 3}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/load_balancer_types"))
            .and(query_param("name", "lb11"))
            .and(query_param("page", "4"))
            .and(query_param("per_page", "15"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "load_balancer_types": [{"id": 4}]
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_floating_ips(Parameters(NameLabelSortPageArgs {
                        name: Some("web-1".to_string()),
                        label_selector: Some("env=prod".to_string()),
                        sort: Some(vec!["id:asc".to_string(), "created:desc".to_string()]),
                        page: Some(1),
                        per_page: Some(25)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"floating_ips": [{"id": 1}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_primary_ips(Parameters(ListPrimaryIpsArgs {
                        name: Some("web-2".to_string()),
                        label_selector: Some("env=dev".to_string()),
                        ip: Some("1.2.3.4".to_string()),
                        sort: Some(vec!["created:desc".to_string()]),
                        page: Some(2),
                        per_page: Some(30)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"primary_ips": [{"id": 2}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_load_balancers(Parameters(NameLabelSortPageArgs {
                        name: Some("lb-1".to_string()),
                        label_selector: Some("team=x".to_string()),
                        sort: Some(vec!["name:desc".to_string()]),
                        page: Some(3),
                        per_page: Some(10)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"load_balancers": [{"id": 3}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_load_balancer_types(Parameters(ListLoadBalancerTypesArgs {
                        name: Some("lb11".to_string()),
                        page: Some(4),
                        per_page: Some(15)
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"load_balancer_types": [{"id": 4}]})
        );
    }

    /// F1: an empty label_selector must be dropped, not sent as
    /// `?label_selector=` (Hetzner 400s on that).
    #[tokio::test]
    async fn list_floating_ips_drops_an_empty_label_selector() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/floating_ips"))
            .and(query_param_is_missing("label_selector"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"floating_ips": []})),
            )
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .list_floating_ips(Parameters(NameLabelSortPageArgs {
                name: None,
                label_selector: Some(String::new()),
                sort: None,
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"floating_ips": []})
        );
    }

    /// F5: an empty `ip` filter must be dropped, not sent as `?ip=`.
    #[tokio::test]
    async fn list_primary_ips_drops_an_empty_ip_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/primary_ips"))
            .and(query_param_is_missing("ip"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"primary_ips": []})),
            )
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .list_primary_ips(Parameters(ListPrimaryIpsArgs {
                name: None,
                label_selector: None,
                ip: Some(String::new()),
                sort: None,
                page: None,
                per_page: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"primary_ips": []})
        );
    }

    /// Every get_* tool builds `/{resource}/{id}` and passes the envelope through.
    /// F2: also proves the id is actually interpolated, not hardcoded - a
    /// request for an unmounted id must fail rather than silently succeed.
    #[tokio::test]
    async fn get_tools_hit_their_id_path_and_return_the_envelope() {
        let server = MockServer::start().await;
        for (route, envelope) in [
            (
                "/floating_ips/42",
                serde_json::json!({"floating_ip": {"id": 42}}),
            ),
            (
                "/primary_ips/7",
                serde_json::json!({"primary_ip": {"id": 7}}),
            ),
            (
                "/load_balancers/3",
                serde_json::json!({"load_balancer": {"id": 3}}),
            ),
            (
                "/load_balancer_types/5",
                serde_json::json!({"load_balancer_type": {"id": 5}}),
            ),
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
                    .get_floating_ip(Parameters(IdArgs { id: 42 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"floating_ip": {"id": 42}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_primary_ip(Parameters(IdArgs { id: 7 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"primary_ip": {"id": 7}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_load_balancer(Parameters(IdArgs { id: 3 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"load_balancer": {"id": 3}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_load_balancer_type(Parameters(IdArgs { id: 5 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"load_balancer_type": {"id": 5}})
        );

        assert_eq!(
            hcloud
                .get_floating_ip(Parameters(IdArgs { id: 999 }))
                .await
                .unwrap()
                .is_error,
            Some(true),
            "get_floating_ip: an unmounted id must not resolve to another tool's route"
        );
        assert_eq!(
            hcloud
                .get_primary_ip(Parameters(IdArgs { id: 999 }))
                .await
                .unwrap()
                .is_error,
            Some(true),
            "get_primary_ip: an unmounted id must not resolve to another tool's route"
        );
        assert_eq!(
            hcloud
                .get_load_balancer(Parameters(IdArgs { id: 999 }))
                .await
                .unwrap()
                .is_error,
            Some(true),
            "get_load_balancer: an unmounted id must not resolve to another tool's route"
        );
        assert_eq!(
            hcloud
                .get_load_balancer_type(Parameters(IdArgs { id: 999 }))
                .await
                .unwrap()
                .is_error,
            Some(true),
            "get_load_balancer_type: an unmounted id must not resolve to another tool's route"
        );
    }

    #[tokio::test]
    async fn get_floating_ip_maps_upstream_errors_to_a_tool_error_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/floating_ips/9"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": "not_found", "message": "floating ip not found"}
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .get_floating_ip(Parameters(IdArgs { id: 9 }))
            .await
            .unwrap();
        assert_eq!(res.is_error, Some(true));
        let v = serde_json::to_value(&res).unwrap();
        let text = v["content"][0]["text"].as_str().expect("text content");
        assert!(text.contains("not_found"), "got: {text}");
    }

    #[test]
    fn net_router_registers_all_eight_tools_with_read_only_annotations() {
        use std::collections::HashSet;

        let router = super::HcloudServer::net_router();
        let names = [
            "list_floating_ips",
            "get_floating_ip",
            "list_primary_ips",
            "get_primary_ip",
            "list_load_balancers",
            "get_load_balancer",
            "list_load_balancer_types",
            "get_load_balancer_type",
        ];
        assert_eq!(router.list_all().len(), 8);
        let mut titles = HashSet::new();
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
            assert_eq!(
                annotations.open_world_hint,
                Some(true),
                "{name} must be open_world_hint = true"
            );
            let title = annotations
                .title
                .as_deref()
                .unwrap_or_else(|| panic!("{name} has no title"));
            assert!(!title.is_empty(), "{name} must have a non-empty title");
            assert!(
                titles.insert(title),
                "{name}'s title {title:?} is not distinct from another tool's"
            );
        }
    }
}
