//! Remaining resource tools: certificates, isos, placement_groups, zones.
//! Implemented in milestone M9b (brief B20); filters and zone-id hardening
//! added in the B28 fix round.
//!
//! Args structs are shared across tools with the same shape, matching the
//! pattern in infra.rs: `IdArgs` for numeric get_*, `NameLabelSortTypePageArgs`
//! for list_* tools whose spec entry lists name/label_selector/sort/type
//! (certificates, placement_groups - the `type` enum differs per resource,
//! but the field doc stays generic, the same tradeoff IdArgs/PageArgs already
//! make). list_zones and list_isos get their own structs since their filter
//! sets (mode; architecture + wildcard) don't match that shape.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{
    HcloudServer, IdArgs, ZoneIdArgs, pagination_query, push_param, respond, validate_zone_id,
};

/// Pagination plus name/label/sort/type filters, shared by list tools whose
/// spec entry lists all four (certificates, placement_groups).
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct NameLabelSortTypePageArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Label selector to filter results, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Sort order, e.g. "id:asc" or "name:desc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Resource type filter; repeatable. Certificates: "uploaded" or
    /// "managed"; placement groups: "spread".
    pub r#type: Option<Vec<String>>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

fn name_label_sort_type_query(
    args: NameLabelSortTypePageArgs,
) -> Result<Vec<(&'static str, String)>, ErrorData> {
    let mut q = pagination_query(args.page, args.per_page)?;
    push_param(&mut q, "name", args.name);
    push_param(&mut q, "label_selector", args.label_selector);
    for sort in args.sort.into_iter().flatten() {
        push_param(&mut q, "sort", Some(sort));
    }
    for t in args.r#type.into_iter().flatten() {
        push_param(&mut q, "type", Some(t));
    }
    Ok(q)
}

/// Pagination plus name/label/sort/mode filters for list_zones.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ZoneListArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// Label selector to filter results, e.g. "env=prod".
    pub label_selector: Option<String>,
    /// Sort order, e.g. "id:asc" or "name:desc"; repeatable.
    pub sort: Option<Vec<String>>,
    /// Zone mode filter: "primary" or "secondary".
    pub mode: Option<String>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

fn zone_list_query(args: ZoneListArgs) -> Result<Vec<(&'static str, String)>, ErrorData> {
    let mut q = pagination_query(args.page, args.per_page)?;
    push_param(&mut q, "name", args.name);
    push_param(&mut q, "label_selector", args.label_selector);
    for sort in args.sort.into_iter().flatten() {
        push_param(&mut q, "sort", Some(sort));
    }
    push_param(&mut q, "mode", args.mode);
    Ok(q)
}

/// Pagination plus name/architecture/wildcard filters for list_isos.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IsoListArgs {
    /// Exact name to filter by.
    pub name: Option<String>,
    /// CPU architecture filter: "x86" or "arm".
    pub architecture: Option<String>,
    /// Also include ISOs with no architecture set when filtering by architecture.
    pub include_architecture_wildcard: Option<bool>,
    /// Page number to fetch, 1-based.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
    /// Results per page, up to 50 (API default 25).
    #[schemars(range(min = 1, max = 50))]
    pub per_page: Option<u32>,
}

fn iso_list_query(args: IsoListArgs) -> Result<Vec<(&'static str, String)>, ErrorData> {
    let mut q = pagination_query(args.page, args.per_page)?;
    push_param(&mut q, "name", args.name);
    push_param(&mut q, "architecture", args.architecture);
    if let Some(wildcard) = args.include_architecture_wildcard {
        push_param(
            &mut q,
            "include_architecture_wildcard",
            Some(wildcard.to_string()),
        );
    }
    Ok(q)
}

#[tool_router(router = misc_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "List TLS certificates, optionally filtered by name, label selector, sort, or type.",
        annotations(
            title = "List certificates",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_certificates(
        &self,
        Parameters(args): Parameters<NameLabelSortTypePageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = name_label_sort_type_query(args)?;
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
        description = "List ISOs available to attach to servers, optionally filtered by name, architecture, or the architecture wildcard flag.",
        annotations(
            title = "List ISOs",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_isos(
        &self,
        Parameters(args): Parameters<IsoListArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = iso_list_query(args)?;
        respond(self.client.get("/isos", &query).await)
    }

    #[tool(
        description = "Get a single ISO by ID.",
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
        description = "List placement groups, optionally filtered by name, label selector, sort, or type.",
        annotations(
            title = "List placement groups",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_placement_groups(
        &self,
        Parameters(args): Parameters<NameLabelSortTypePageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = name_label_sort_type_query(args)?;
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
        description = "List DNS Zones, optionally filtered by name, label selector, sort, or mode.",
        annotations(
            title = "List Zones",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    async fn list_zones(
        &self,
        Parameters(args): Parameters<ZoneListArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = zone_list_query(args)?;
        respond(self.client.get("/zones", &query).await)
    }

    #[tool(
        description = "Get a single DNS Zone by ID or name.",
        annotations(
            title = "Get Zone",
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
    use super::{IdArgs, IsoListArgs, NameLabelSortTypePageArgs, ZoneIdArgs, ZoneListArgs};

    /// list_certificates and list_placement_groups forward page/per_page,
    /// name, label_selector, repeated sort, and repeated type verbatim, and
    /// pass the response envelope through.
    #[tokio::test]
    async fn certificates_and_placement_groups_forward_all_filters_including_type() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/certificates"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "25"))
            .and(query_param("name", "cert-1"))
            .and(query_param("label_selector", "env=prod"))
            .and(query_param("sort", "id:asc"))
            .and(query_param("type", "uploaded"))
            .and(query_param("type", "managed"))
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
            .and(query_param("type", "spread"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "placement_groups": [{"id": 2}]
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_certificates(Parameters(NameLabelSortTypePageArgs {
                        name: Some("cert-1".into()),
                        label_selector: Some("env=prod".into()),
                        sort: Some(vec!["id:asc".into()]),
                        r#type: Some(vec!["uploaded".into(), "managed".into()]),
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
                    .list_placement_groups(Parameters(NameLabelSortTypePageArgs {
                        name: Some("pg-1".into()),
                        label_selector: Some("team=x".into()),
                        sort: Some(vec!["created:desc".into()]),
                        r#type: Some(vec!["spread".into()]),
                        page: Some(2),
                        per_page: Some(30),
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"placement_groups": [{"id": 2}]})
        );
    }

    /// list_zones forwards page/per_page, name, label_selector, sort, and mode.
    #[tokio::test]
    async fn list_zones_forwards_all_filters_including_mode() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zones"))
            .and(query_param("page", "3"))
            .and(query_param("per_page", "10"))
            .and(query_param("name", "example.com"))
            .and(query_param("label_selector", "app=y"))
            .and(query_param("sort", "name:asc"))
            .and(query_param("mode", "primary"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "zones": [{"id": 3}]
            })))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_zones(Parameters(ZoneListArgs {
                        name: Some("example.com".into()),
                        label_selector: Some("app=y".into()),
                        sort: Some(vec!["name:asc".into()]),
                        mode: Some("primary".into()),
                        page: Some(3),
                        per_page: Some(10),
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"zones": [{"id": 3}]})
        );
    }

    /// list_isos forwards name, architecture, and the wildcard flag (as a
    /// string), and omits architecture/wildcard when unset.
    #[tokio::test]
    async fn list_isos_forwards_name_architecture_and_wildcard() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/isos"))
            .and(query_param("page", "1"))
            .and(query_param("per_page", "25"))
            .and(query_param("name", "fedora-40"))
            .and(query_param("architecture", "arm"))
            .and(query_param("include_architecture_wildcard", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "isos": [{"id": 4}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/isos"))
            .and(query_param_is_missing("architecture"))
            .and(query_param_is_missing("include_architecture_wildcard"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"isos": []})))
            .mount(&server)
            .await;

        let hcloud = server_for(server.uri());
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_isos(Parameters(IsoListArgs {
                        name: Some("fedora-40".into()),
                        architecture: Some("arm".into()),
                        include_architecture_wildcard: Some(true),
                        page: Some(1),
                        per_page: Some(25),
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"isos": [{"id": 4}]})
        );
        assert_eq!(
            tool_result_json(
                &hcloud
                    .list_isos(Parameters(IsoListArgs {
                        name: None,
                        architecture: None,
                        include_architecture_wildcard: None,
                        page: None,
                        per_page: None,
                    }))
                    .await
                    .unwrap()
            ),
            serde_json::json!({"isos": []})
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
            .list_certificates(Parameters(NameLabelSortTypePageArgs {
                name: None,
                label_selector: Some(String::new()),
                sort: None,
                r#type: None,
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
    /// tool's route (proves the id is actually interpolated, not hardcoded -
    /// F2: this used to be asserted only for get_certificate).
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

        for (label, unmounted) in [
            (
                "get_certificate",
                hcloud
                    .get_certificate(Parameters(IdArgs { id: 999 }))
                    .await
                    .unwrap(),
            ),
            (
                "get_iso",
                hcloud
                    .get_iso(Parameters(IdArgs { id: 999 }))
                    .await
                    .unwrap(),
            ),
            (
                "get_placement_group",
                hcloud
                    .get_placement_group(Parameters(IdArgs { id: 999 }))
                    .await
                    .unwrap(),
            ),
        ] {
            assert_eq!(
                unmounted.is_error,
                Some(true),
                "{label}: an id with no mounted mock must not resolve to another tool's route"
            );
        }
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
    /// mounted and any success would surface as an HTTP failure (isError),
    /// not this Err. Covers F1 (".", "..", "a..b" retarget the URL to the
    /// collection endpoint - wire-confirmed for "."), F3 (asserts the
    /// protocol error, not an isError result), and F4 (length bound).
    #[tokio::test]
    async fn get_zone_rejects_invalid_ids_without_making_a_request() {
        let server = MockServer::start().await;
        let hcloud = server_for(server.uri());
        let too_long = "a".repeat(254);

        for bad in [
            "../servers/1",
            "a?x",
            "",
            "zone#1",
            "ex ample.com",
            ".",
            "..",
            "a..b",
            too_long.as_str(),
        ] {
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

    /// F5: every tool must also carry open_world_hint = true and a distinct,
    /// non-empty title, not just the read_only/destructive hints.
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
        let mut titles = Vec::with_capacity(names.len());
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
                .clone()
                .unwrap_or_else(|| panic!("{name} has no title"));
            assert!(!title.is_empty(), "{name} title must not be empty");
            titles.push(title);
        }
        let mut unique_titles = titles.clone();
        unique_titles.sort();
        unique_titles.dedup();
        assert_eq!(
            unique_titles.len(),
            titles.len(),
            "all tool titles must be distinct, got: {titles:?}"
        );
    }
}
