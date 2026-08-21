//! Remaining resource tools: certificates, isos, placement_groups, zones.
//! Implemented in milestone M9b (brief B20).
//!
//! Args structs are shared across tools with the same shape, matching the
//! pattern in infra.rs: `IdArgs` for numeric get_*, `NameLabelSortPageArgs`
//! for list_* tools whose spec entry lists name/label_selector/sort
//! (certificates, placement_groups, zones), `NamePageArgs` for list_isos
//! (the spec gives isos no label_selector or sort parameter).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{HcloudServer, pagination_query, push_param, respond};

/// Numeric ID of the resource to fetch.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IdArgs {
    /// Numeric ID of the resource, from the matching list_* tool's response.
    pub id: u64,
}

/// Pagination plus name/label/sort filters, shared by list tools whose spec
/// entry lists all three (certificates, placement_groups, zones).
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NameLabelSortPageArgs {
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

fn name_label_sort_page_query(args: NameLabelSortPageArgs) -> Vec<(&'static str, String)> {
    let mut q = pagination_query(args.page, args.per_page);
    push_param(&mut q, "name", args.name);
    push_param(&mut q, "label_selector", args.label_selector);
    for sort in args.sort.into_iter().flatten() {
        push_param(&mut q, "sort", Some(sort));
    }
    q
}

/// Pagination plus name filter, for list_isos.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NamePageArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

/// ID or name of a zone - the only string this crate interpolates into a URL path.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ZoneIdArgs {
    /// Zone ID or name, from list_zones. ASCII letters, digits, '.', and '-' only.
    pub id_or_name: String,
}

/// Reject anything but a bare path segment before it reaches the URL. This is
/// the only string this crate puts in a request path, so an unvalidated value
/// (e.g. "../servers/1") could redirect the request to a different endpoint.
fn validate_zone_id(id_or_name: &str) -> Result<(), ErrorData> {
    let valid = !id_or_name.is_empty()
        && id_or_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if valid {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            "id_or_name must be non-empty and contain only ASCII letters, digits, '.', or '-'",
            None,
        ))
    }
}

#[tool_router(router = misc_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "List TLS certificates, optionally filtered by name, label selector, or sort.",
        annotations(
            title = "List certificates",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_certificates(
        &self,
        Parameters(args): Parameters<NameLabelSortPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = name_label_sort_page_query(args);
        respond(self.client.get("/certificates", &query).await)
    }

    #[tool(
        description = "Get a single TLS certificate by ID.",
        annotations(
            title = "Get certificate",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_certificate(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.client
                .get(&format!("/certificates/{}", args.id), &[])
                .await,
        )
    }

    #[tool(
        description = "List ISO images available to attach to servers, optionally filtered by name.",
        annotations(
            title = "List ISOs",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_isos(
        &self,
        Parameters(args): Parameters<NamePageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut query = pagination_query(args.page, args.per_page);
        push_param(&mut query, "name", args.name);
        respond(self.client.get("/isos", &query).await)
    }

    #[tool(
        description = "Get a single ISO image by ID.",
        annotations(
            title = "Get ISO",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_iso(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.get(&format!("/isos/{}", args.id), &[]).await)
    }

    #[tool(
        description = "List placement groups, optionally filtered by name, label selector, or sort.",
        annotations(
            title = "List placement groups",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_placement_groups(
        &self,
        Parameters(args): Parameters<NameLabelSortPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = name_label_sort_page_query(args);
        respond(self.client.get("/placement_groups", &query).await)
    }

    #[tool(
        description = "Get a single placement group by ID.",
        annotations(
            title = "Get placement group",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_placement_group(
        &self,
        Parameters(args): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(
            self.client
                .get(&format!("/placement_groups/{}", args.id), &[])
                .await,
        )
    }

    #[tool(
        description = "List DNS zones, optionally filtered by name, label selector, or sort.",
        annotations(
            title = "List zones",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_zones(
        &self,
        Parameters(args): Parameters<NameLabelSortPageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = name_label_sort_page_query(args);
        respond(self.client.get("/zones", &query).await)
    }

    #[tool(
        description = "Get a single DNS zone by ID or name.",
        annotations(
            title = "Get zone",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn get_zone(
        &self,
        Parameters(args): Parameters<ZoneIdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_zone_id(&args.id_or_name)?;
        respond(
            self.client
                .get(&format!("/zones/{}", args.id_or_name), &[])
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
    use super::{IdArgs, NameLabelSortPageArgs, NamePageArgs, ZoneIdArgs};

    /// Every sort-capable list_* tool forwards page/per_page, name,
    /// label_selector, and repeated sort verbatim, and passes the envelope through.
    #[tokio::test]
    async fn sort_capable_list_tools_forward_all_filters_and_return_the_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/certificates"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "25"))
            .and(query_param("name", "cert-1"))
            .and(query_param("label_selector", "env=prod"))
            .and(query_param("sort", "id:asc"))
            .and(query_param("sort", "name:desc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "certificates": [{"id": 1}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/placement_groups"))
            .and(query_param("page", "2"))
            .and(query_param("per_page", "30"))
            .and(query_param("name", "pg-1"))
            .and(query_param("label_selector", "team=x"))
            .and(query_param("sort", "created:desc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "placement_groups": [{"id": 2}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/zones"))
            .and(query_param("page", "3"))
            .and(query_param("per_page", "10"))
            .and(query_param("name", "example.com"))
            .and(query_param("label_selector", "app=y"))
            .and(query_param("sort", "name:asc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "zones": [{"id": 3}]
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_certificates(Parameters(NameLabelSortPageArgs {
                        name: Some("cert-1".into()),
                        label_selector: Some("env=prod".into()),
                        sort: Some(vec!["id:asc".into(), "name:desc".into()]),
                        page: Some(1),
                        per_page: Some(25),
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"certificates": [{"id": 1}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_placement_groups(Parameters(NameLabelSortPageArgs {
                        name: Some("pg-1".into()),
                        label_selector: Some("team=x".into()),
                        sort: Some(vec!["created:desc".into()]),
                        page: Some(2),
                        per_page: Some(30),
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"placement_groups": [{"id": 2}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_zones(Parameters(NameLabelSortPageArgs {
                        name: Some("example.com".into()),
                        label_selector: Some("app=y".into()),
                        sort: Some(vec!["name:asc".into()]),
                        page: Some(3),
                        per_page: Some(10),
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"zones": [{"id": 3}]})
        );
    }

    /// list_isos only supports name + pagination per the spec; no
    /// label_selector or sort param should ever be sent for it.
    #[tokio::test]
    async fn list_isos_forwards_name_and_pagination_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/isos"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "25"))
            .and(query_param("name", "fedora-40"))
            .and(query_param_is_missing("label_selector"))
            .and(query_param_is_missing("sort"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "isos": [{"id": 4}]
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_isos(Parameters(NamePageArgs {
                        name: Some("fedora-40".into()),
                        page: Some(1),
                        per_page: Some(25),
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"isos": [{"id": 4}]})
        );
    }

    /// An empty label_selector must be dropped, not sent as `?label_selector=`.
    #[tokio::test]
    async fn list_certificates_drops_an_empty_label_selector() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/certificates"))
            .and(query_param_is_missing("label_selector"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"certificates": []})),
            )
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .list_certificates(Parameters(NameLabelSortPageArgs {
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
            serde_json::json!({"certificates": []})
        );
    }

    /// Every get_* tool builds `/{resource}/{id}` and passes the envelope
    /// through; an id with no mounted mock must not resolve to another
    /// tool's route (proves the id is actually interpolated).
    #[tokio::test]
    async fn get_tools_hit_their_id_path_and_return_the_envelope() {
        let server = MockServer::start().await;
        for (route, envelope) in [
            (
                "/certificates/42",
                serde_json::json!({"certificate": {"id": 42}}),
            ),
            ("/isos/7", serde_json::json!({"iso": {"id": 7}})),
            (
                "/placement_groups/3",
                serde_json::json!({"placement_group": {"id": 3}}),
            ),
            (
                "/zones/example.com",
                serde_json::json!({"zone": {"id": 5, "name": "example.com"}}),
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
                    .get_certificate(Parameters(IdArgs { id: 42 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"certificate": {"id": 42}})
        );
        assert_eq!(
            tool_result_json(&hcloud.get_iso(Parameters(IdArgs { id: 7 })).await.unwrap()),
            serde_json::json!({"iso": {"id": 7}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_placement_group(Parameters(IdArgs { id: 3 }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"placement_group": {"id": 3}})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .get_zone(Parameters(ZoneIdArgs {
                        id_or_name: "example.com".into()
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"zone": {"id": 5, "name": "example.com"}})
        );

        let unmounted = hcloud
            .get_certificate(Parameters(IdArgs { id: 999 }))
            .await
            .unwrap();
        assert_eq!(
            unmounted.is_error,
            Some(true),
            "an id with no mounted mock must not resolve to another tool's route"
        );
    }

    #[tokio::test]
    async fn get_certificate_maps_upstream_errors_to_a_tool_error_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/certificates/9"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": {"code": "not_found", "message": "certificate not found"}
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .get_certificate(Parameters(IdArgs { id: 9 }))
            .await
            .unwrap();
        assert_eq!(res.is_error, Some(true));
        let v = serde_json::to_value(&res).unwrap();
        let text = v["content"][0]["text"].as_str().expect("text content");
        assert!(text.contains("not_found"), "got: {text}");
    }

    /// get_zone must validate id_or_name BEFORE building the request path -
    /// none of these bad values may ever reach the HTTP layer, so no mock is
    /// mounted and success would surface as an HTTP failure, not this Err.
    #[tokio::test]
    async fn get_zone_rejects_invalid_ids_without_making_a_request() {
        let server = MockServer::start().await;
        let hcloud = server_for(server.uri());

        for bad in ["../servers/1", "a?x", "", "zone#1", "ex ample.com"] {
            let err = hcloud
                .get_zone(Parameters(ZoneIdArgs {
                    id_or_name: bad.to_string(),
                }))
                .await
                .expect_err(&format!("{bad:?} must be rejected"));
            assert!(
                err.message.contains("id_or_name"),
                "error for {bad:?} must name the rule, got: {}",
                err.message
            );
        }
    }

    #[tokio::test]
    async fn get_zone_accepts_a_valid_id_or_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zones/my-zone.example-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "zone": {"id": 1, "name": "my-zone.example-1"}
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        let res = hcloud
            .get_zone(Parameters(ZoneIdArgs {
                id_or_name: "my-zone.example-1".into(),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"zone": {"id": 1, "name": "my-zone.example-1"}})
        );
    }

    #[test]
    fn misc_router_registers_all_eight_tools_with_read_only_annotations() {
        let router = super::HcloudServer::misc_router();
        let names = [
            "list_certificates",
            "get_certificate",
            "list_isos",
            "get_iso",
            "list_placement_groups",
            "get_placement_group",
            "list_zones",
            "get_zone",
        ];
        assert_eq!(router.list_all().len(), 8);
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
