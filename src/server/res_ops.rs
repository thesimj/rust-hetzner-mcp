//! Image/ssh_key/volume/placement_group/certificate mutations (M15b, brief B24;
//! test-suite fixes B33).
//!
//! `IdArgs` and `UpdateNameLabelsArgs` are shared across tools with the same
//! shape (delete-by-id; update name+labels) rather than one struct per tool,
//! matching compute.rs's/infra.rs's convention. `id` on the update structs is
//! `skip_serializing` so it stays a required *input* field (visible in the
//! JSON schema) but never leaks into the PUT body.

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::schemars::JsonSchema;
use rmcp::{ErrorData, tool, tool_router};
use serde::{Deserialize, Serialize};

use super::{
    ActionArgs, HcloudServer, IdArgs, action_body, check_action, require_update_fields, respond,
};

const IMAGE_ACTIONS: [&str; 1] = ["change_protection"];
const VOLUME_ACTIONS: [&str; 4] = ["attach", "detach", "resize", "change_protection"];
const CERTIFICATE_ACTIONS: [&str; 1] = ["retry"];

/// Body shared by every update tool that only ever sets `name`/`labels`
/// (ssh_key, volume, placement_group, certificate).
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateNameLabelsArgs {
    /// Numeric ID of the resource to update.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New name to set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Labels to set on the resource (replaces existing labels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct UpdateImageArgs {
    /// Numeric ID of the image to update.
    #[serde(skip_serializing)]
    pub id: u64,
    /// New description of the image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Destination image type to convert to; the only accepted value is "snapshot".
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Labels to set on the image (replaces existing labels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateVolumeArgs {
    /// Size of the volume in GB.
    pub size: u64,
    /// Name of the volume.
    pub name: String,
    /// Location to create the volume in (omit if `server` is given).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Server ID to attach the volume to once created (created in that server's location).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<u64>,
    /// Auto-mount the volume after attach; requires `server`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automount: Option<bool>,
    /// Format the volume after creation: "xfs" or "ext4".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Labels to attach to the volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreatePlacementGroupArgs {
    /// Name of the placement group.
    pub name: String,
    /// Placement group type; the only accepted value is "spread".
    #[serde(rename = "type")]
    pub r#type: String,
    /// Labels to attach to the placement group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct CreateCertificateArgs {
    /// Name of the certificate.
    pub name: String,
    /// "uploaded" to provide `certificate`/`private_key`, or "managed" to request one from Let's Encrypt.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Certificate and chain in PEM format. Required for type "uploaded".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,
    /// Certificate private key in PEM format. Required for type "uploaded".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// Domains to include. Required for type "managed".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_names: Option<Vec<String>>,
    /// Labels to attach to the certificate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeMap<String, String>>,
}

#[tool_router(router = res_ops_router, vis = "pub(crate)")]
impl HcloudServer {
    #[tool(
        description = "Update an image's description, type, or labels. Labels replace \
        the full existing set, not a merge.",
        annotations(
            title = "Update image",
            idempotent_hint = true,
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_image(
        &self,
        Parameters(args): Parameters<UpdateImageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&format!("/images/{id}"), body).await)
    }

    #[tool(
        description = "Delete an image permanently. Only snapshots and backups can be deleted.",
        annotations(
            title = "Delete image",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_image(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/images/{id}")).await)
    }

    #[tool(
        description = "Run an image action: change_protection, which toggles delete protection.",
        annotations(
            title = "Run image action",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn image_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        check_action(&IMAGE_ACTIONS, &args.action)?;
        respond(
            self.client
                .post(
                    &format!("/images/{}/actions/{}", args.id, args.action),
                    action_body(args.params),
                )
                .await,
        )
    }

    #[tool(
        description = "Update an SSH key's name or labels. Labels replace the full \
        existing set, not a merge.",
        annotations(
            title = "Update SSH key",
            idempotent_hint = true,
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_ssh_key(
        &self,
        Parameters(args): Parameters<UpdateNameLabelsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&format!("/ssh_keys/{id}"), body).await)
    }

    #[tool(
        description = "Create a new block storage volume. This creates a BILLABLE resource. \
        location is required unless server is given (the volume is then created in that \
        server's location).",
        annotations(
            title = "Create volume",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_volume(
        &self,
        Parameters(args): Parameters<CreateVolumeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.location.is_none() && args.server.is_none() {
            return Err(ErrorData::invalid_params(
                "location is required when server is not given",
                None,
            ));
        }
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/volumes", body).await)
    }

    #[tool(
        description = "Update a volume's name or labels. Labels replace the full \
        existing set, not a merge.",
        annotations(
            title = "Update volume",
            idempotent_hint = true,
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_volume(
        &self,
        Parameters(args): Parameters<UpdateNameLabelsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&format!("/volumes/{id}"), body).await)
    }

    #[tool(
        description = "Delete a volume permanently. It must be detached from any server first.",
        annotations(
            title = "Delete volume",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_volume(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/volumes/{id}")).await)
    }

    #[tool(
        description = "Run a volume action: attach requires params {\"server\": <server_id>} \
        (optional \"automount\"); resize requires params {\"size\": <new_size_gb>} and can only \
        increase the size, which cannot be undone; detach and change_protection take no/optional params.",
        annotations(
            title = "Run volume action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn volume_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        check_action(&VOLUME_ACTIONS, &args.action)?;
        respond(
            self.client
                .post(
                    &format!("/volumes/{}/actions/{}", args.id, args.action),
                    action_body(args.params),
                )
                .await,
        )
    }

    #[tool(
        description = "Create a new placement group to control server co-location.",
        annotations(
            title = "Create placement group",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_placement_group(
        &self,
        Parameters(args): Parameters<CreatePlacementGroupArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/placement_groups", body).await)
    }

    #[tool(
        description = "Update a placement group's name or labels. Labels replace the \
        full existing set, not a merge.",
        annotations(
            title = "Update placement group",
            idempotent_hint = true,
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_placement_group(
        &self,
        Parameters(args): Parameters<UpdateNameLabelsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(
            self.client
                .put(&format!("/placement_groups/{id}"), body)
                .await,
        )
    }

    #[tool(
        description = "Delete a placement group permanently.",
        annotations(
            title = "Delete placement group",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_placement_group(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/placement_groups/{id}")).await)
    }

    #[tool(
        description = "Upload a certificate, or request a managed Let's Encrypt certificate. \
        type \"uploaded\" requires certificate and private_key; type \"managed\" requires \
        domain_names.",
        annotations(
            title = "Create certificate",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn create_certificate(
        &self,
        Parameters(args): Parameters<CreateCertificateArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match args.r#type.as_deref() {
            Some("uploaded") if args.certificate.is_none() || args.private_key.is_none() => {
                return Err(ErrorData::invalid_params(
                    "certificate and private_key are required for type \"uploaded\"",
                    None,
                ));
            }
            Some("managed") if args.domain_names.as_ref().is_none_or(|d| d.is_empty()) => {
                return Err(ErrorData::invalid_params(
                    "domain_names is required for type \"managed\"",
                    None,
                ));
            }
            _ => {}
        }
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/certificates", body).await)
    }

    #[tool(
        description = "Update a certificate's name or labels. Labels replace the full \
        existing set, not a merge.",
        annotations(
            title = "Update certificate",
            idempotent_hint = true,
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn update_certificate(
        &self,
        Parameters(args): Parameters<UpdateNameLabelsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args.id;
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        require_update_fields(&body)?;
        respond(self.client.put(&format!("/certificates/{id}"), body).await)
    }

    #[tool(
        description = "Delete a certificate permanently.",
        annotations(
            title = "Delete certificate",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn delete_certificate(
        &self,
        Parameters(IdArgs { id }): Parameters<IdArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        respond(self.client.delete(&format!("/certificates/{id}")).await)
    }

    #[tool(
        description = "Run a certificate action: retry, which retries issuance for a \
        failed managed certificate.",
        annotations(
            title = "Run certificate action",
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = true
        )
    )]
    pub(crate) async fn certificate_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        check_action(&CERTIFICATE_ACTIONS, &args.action)?;
        respond(
            self.client
                .post(
                    &format!("/certificates/{}/actions/{}", args.id, args.action),
                    action_body(args.params),
                )
                .await,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::ErrorCode;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::server::test_support::{server_for, tool_result_json};

    fn map_of(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        v.as_object().expect("object literal").clone()
    }

    /// Mount one row's mock (method/path/body), invoke `call`, and assert the
    /// row's own distinct response. Every row uses a distinct id (so a
    /// hardcoded-id mutation 404s), a distinct path (so a path-swap mutation
    /// 404s), and a distinct response payload (so a mock mix-up fails the
    /// `assert_eq!` even if two paths happened to collide).
    async fn assert_binds_its_own_path<Fut>(
        mock: &MockServer,
        http_method: &str,
        expected_path: &str,
        expected_body: Option<serde_json::Value>,
        response: serde_json::Value,
        call: impl FnOnce() -> Fut,
    ) where
        Fut: Future<Output = Result<CallToolResult, ErrorData>>,
    {
        let mut builder = Mock::given(method(http_method)).and(path(expected_path));
        if let Some(body) = expected_body {
            builder = builder.and(body_json(body));
        }
        builder
            .respond_with(ResponseTemplate::new(200).set_body_json(&response))
            .mount(mock)
            .await;
        let res = call().await.unwrap();
        assert_eq!(tool_result_json(&res), response);
    }

    /// F1/F3/F4: table-driven, one row per id-interpolating tool (12 tools;
    /// volume_action gets a row per action, so 15 rows), all sharing one
    /// MockServer. Each row's id/path/payload is unique, so a path-swap
    /// (e.g. update_volume -> /ssh_keys/{id}) or a hardcoded-id regression in
    /// ANY row 404s instead of silently matching a sibling row's mock.
    #[tokio::test]
    async fn every_id_interpolating_tool_binds_its_own_path_and_id() {
        let mock = MockServer::start().await;
        let server = server_for(mock.uri());
        let row = |id: u64| serde_json::json!({"row": id});

        assert_binds_its_own_path(
            &mock,
            "PUT",
            "/images/201",
            Some(serde_json::json!({"description": "d"})),
            row(201),
            || {
                server.update_image(Parameters(UpdateImageArgs {
                    id: 201,
                    description: Some("d".into()),
                    r#type: None,
                    labels: None,
                }))
            },
        )
        .await;

        assert_binds_its_own_path(&mock, "DELETE", "/images/202", None, row(202), || {
            server.delete_image(Parameters(IdArgs { id: 202 }))
        })
        .await;

        assert_binds_its_own_path(
            &mock,
            "POST",
            "/images/203/actions/change_protection",
            Some(serde_json::json!({"delete": true})),
            row(203),
            || {
                server.image_action(Parameters(ActionArgs {
                    id: 203,
                    action: "change_protection".into(),
                    params: Some(map_of(serde_json::json!({"delete": true}))),
                }))
            },
        )
        .await;

        assert_binds_its_own_path(
            &mock,
            "PUT",
            "/ssh_keys/204",
            Some(serde_json::json!({"name": "n"})),
            row(204),
            || {
                server.update_ssh_key(Parameters(UpdateNameLabelsArgs {
                    id: 204,
                    name: Some("n".into()),
                    labels: None,
                }))
            },
        )
        .await;

        assert_binds_its_own_path(
            &mock,
            "PUT",
            "/volumes/205",
            Some(serde_json::json!({"name": "n"})),
            row(205),
            || {
                server.update_volume(Parameters(UpdateNameLabelsArgs {
                    id: 205,
                    name: Some("n".into()),
                    labels: None,
                }))
            },
        )
        .await;

        assert_binds_its_own_path(&mock, "DELETE", "/volumes/206", None, row(206), || {
            server.delete_volume(Parameters(IdArgs { id: 206 }))
        })
        .await;

        // volume_action: all four actions, each its own row/path.
        assert_binds_its_own_path(
            &mock,
            "POST",
            "/volumes/207/actions/attach",
            Some(serde_json::json!({"server": 999})),
            row(207),
            || {
                server.volume_action(Parameters(ActionArgs {
                    id: 207,
                    action: "attach".into(),
                    params: Some(map_of(serde_json::json!({"server": 999}))),
                }))
            },
        )
        .await;
        assert_binds_its_own_path(
            &mock,
            "POST",
            "/volumes/208/actions/detach",
            Some(serde_json::json!({})),
            row(208),
            || {
                server.volume_action(Parameters(ActionArgs {
                    id: 208,
                    action: "detach".into(),
                    params: None,
                }))
            },
        )
        .await;
        assert_binds_its_own_path(
            &mock,
            "POST",
            "/volumes/209/actions/resize",
            Some(serde_json::json!({"size": 500})),
            row(209),
            || {
                server.volume_action(Parameters(ActionArgs {
                    id: 209,
                    action: "resize".into(),
                    params: Some(map_of(serde_json::json!({"size": 500}))),
                }))
            },
        )
        .await;
        assert_binds_its_own_path(
            &mock,
            "POST",
            "/volumes/210/actions/change_protection",
            Some(serde_json::json!({"delete": true})),
            row(210),
            || {
                server.volume_action(Parameters(ActionArgs {
                    id: 210,
                    action: "change_protection".into(),
                    params: Some(map_of(serde_json::json!({"delete": true}))),
                }))
            },
        )
        .await;

        assert_binds_its_own_path(
            &mock,
            "PUT",
            "/placement_groups/211",
            Some(serde_json::json!({"name": "n"})),
            row(211),
            || {
                server.update_placement_group(Parameters(UpdateNameLabelsArgs {
                    id: 211,
                    name: Some("n".into()),
                    labels: None,
                }))
            },
        )
        .await;

        assert_binds_its_own_path(
            &mock,
            "DELETE",
            "/placement_groups/212",
            None,
            row(212),
            || server.delete_placement_group(Parameters(IdArgs { id: 212 })),
        )
        .await;

        assert_binds_its_own_path(
            &mock,
            "PUT",
            "/certificates/213",
            Some(serde_json::json!({"name": "n"})),
            row(213),
            || {
                server.update_certificate(Parameters(UpdateNameLabelsArgs {
                    id: 213,
                    name: Some("n".into()),
                    labels: None,
                }))
            },
        )
        .await;

        assert_binds_its_own_path(&mock, "DELETE", "/certificates/214", None, row(214), || {
            server.delete_certificate(Parameters(IdArgs { id: 214 }))
        })
        .await;

        assert_binds_its_own_path(
            &mock,
            "POST",
            "/certificates/215/actions/retry",
            Some(serde_json::json!({})),
            row(215),
            || {
                server.certificate_action(Parameters(ActionArgs {
                    id: 215,
                    action: "retry".into(),
                    params: None,
                }))
            },
        )
        .await;
    }

    #[tokio::test]
    async fn create_volume_sends_exactly_the_required_fields_when_server_covers_location() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/volumes"))
            .and(body_json(
                serde_json::json!({"size": 20, "name": "data-1", "server": 7}),
            ))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"volume": {"id": 1}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_volume(Parameters(CreateVolumeArgs {
                size: 20,
                name: "data-1".into(),
                location: None,
                server: Some(7),
                automount: None,
                format: None,
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"volume": {"id": 1}})
        );
    }

    /// W1.2: location is required when server is not given - rejected before
    /// any request is attempted (dead-port proof).
    #[tokio::test]
    async fn create_volume_rejects_when_neither_location_nor_server_is_given() {
        let err = server_for("http://127.0.0.1:9".to_string())
            .create_volume(Parameters(CreateVolumeArgs {
                size: 20,
                name: "data-1".into(),
                location: None,
                server: None,
                automount: None,
                format: None,
                labels: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn create_volume_forwards_optional_fields() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/volumes"))
            .and(body_json(serde_json::json!({
                "size": 50,
                "name": "data-2",
                "location": "fsn1",
                "server": 7,
                "automount": true,
                "format": "ext4",
                "labels": {"env": "prod"}
            })))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"volume": {"id": 2}})),
            )
            .mount(&mock)
            .await;

        let mut labels = BTreeMap::new();
        labels.insert("env".to_string(), "prod".to_string());
        let server = server_for(mock.uri());
        let res = server
            .create_volume(Parameters(CreateVolumeArgs {
                size: 50,
                name: "data-2".into(),
                location: Some("fsn1".into()),
                server: Some(7),
                automount: Some(true),
                format: Some("ext4".into()),
                labels: Some(labels),
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"volume": {"id": 2}})
        );
    }

    #[tokio::test]
    async fn create_placement_group_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/placement_groups"))
            .and(body_json(
                serde_json::json!({"name": "pg-1", "type": "spread"}),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "placement_group": {"id": 1}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_placement_group(Parameters(CreatePlacementGroupArgs {
                name: "pg-1".into(),
                r#type: "spread".into(),
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"placement_group": {"id": 1}})
        );
    }

    #[tokio::test]
    async fn create_certificate_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/certificates"))
            .and(body_json(serde_json::json!({"name": "cert-1"})))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "certificate": {"id": 1}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_certificate(Parameters(CreateCertificateArgs {
                name: "cert-1".into(),
                r#type: None,
                certificate: None,
                private_key: None,
                domain_names: None,
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"certificate": {"id": 1}})
        );
    }

    #[tokio::test]
    async fn create_certificate_forwards_optional_fields() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/certificates"))
            .and(body_json(serde_json::json!({
                "name": "cert-2",
                "type": "managed",
                "domain_names": ["example.com"]
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "certificate": {"id": 2}
            })))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .create_certificate(Parameters(CreateCertificateArgs {
                name: "cert-2".into(),
                r#type: Some("managed".into()),
                certificate: None,
                private_key: None,
                domain_names: Some(vec!["example.com".into()]),
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"certificate": {"id": 2}})
        );
    }

    /// W1.2: type "uploaded" without certificate/private_key is rejected
    /// before any request is attempted (dead-port proof).
    #[tokio::test]
    async fn create_certificate_rejects_uploaded_without_certificate_and_key() {
        let err = server_for("http://127.0.0.1:9".to_string())
            .create_certificate(Parameters(CreateCertificateArgs {
                name: "cert-3".into(),
                r#type: Some("uploaded".into()),
                certificate: None,
                private_key: None,
                domain_names: None,
                labels: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    /// W1.2: type "managed" without domain_names is rejected before any
    /// request is attempted (dead-port proof).
    #[tokio::test]
    async fn create_certificate_rejects_managed_without_domain_names() {
        let err = server_for("http://127.0.0.1:9".to_string())
            .create_certificate(Parameters(CreateCertificateArgs {
                name: "cert-4".into(),
                r#type: Some("managed".into()),
                certificate: None,
                private_key: None,
                domain_names: None,
                labels: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    /// F2: pins each allowlist to its exact expected array (via `.to_vec()`
    /// so an appended/dropped element fails at runtime, not just diverges by
    /// silently changing the const's inferred length).
    #[test]
    fn action_allowlists_are_pinned_to_their_exact_expected_sets() {
        assert_eq!(IMAGE_ACTIONS.to_vec(), vec!["change_protection"]);
        assert_eq!(
            VOLUME_ACTIONS.to_vec(),
            vec!["attach", "detach", "resize", "change_protection"]
        );
        assert_eq!(CERTIFICATE_ACTIONS.to_vec(), vec!["retry"]);
    }

    /// F2: cross-rejection - each tool must reject an action that's valid on
    /// a *different* resource, proving the allowlist check is resource-exact
    /// rather than "is this a known action anywhere".
    #[tokio::test]
    async fn action_tools_reject_actions_that_only_belong_to_a_sibling_resource() {
        let server = server_for("http://127.0.0.1:9".to_string());

        let err = server
            .image_action(Parameters(ActionArgs {
                id: 1,
                action: "attach".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);

        let err = server
            .volume_action(Parameters(ActionArgs {
                id: 1,
                action: "retry".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);

        let err = server
            .certificate_action(Parameters(ActionArgs {
                id: 1,
                action: "change_protection".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn res_ops_router_registers_all_15_tools_with_expected_annotations() {
        let router = super::HcloudServer::res_ops_router();
        let expected: [(&str, bool, bool); 15] = [
            ("update_image", false, false),
            ("delete_image", false, true),
            ("image_action", false, false),
            ("update_ssh_key", false, false),
            ("create_volume", false, false),
            ("update_volume", false, false),
            ("delete_volume", false, true),
            ("volume_action", false, true),
            ("create_placement_group", false, false),
            ("update_placement_group", false, false),
            ("delete_placement_group", false, true),
            ("create_certificate", false, false),
            ("update_certificate", false, false),
            ("delete_certificate", false, true),
            ("certificate_action", false, false),
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
            // F7: every tool is open-world (talks to the real Hetzner API),
            // and carries a distinct, non-empty title.
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
            if let Some(base) = name.strip_prefix("update_") {
                assert!(
                    title.starts_with("Update"),
                    "{name} (updates {base}) title {title:?} must start with \"Update\""
                );
            }
            assert!(
                titles.insert(title.clone()),
                "title {title:?} reused by more than one tool"
            );
        }
        assert_eq!(titles.len(), 15, "every tool must have a distinct title");

        // F6: create_volume's BILLABLE warning must actually be present, not
        // just written once and left to bit-rot unasserted.
        let create_volume_description = router
            .get("create_volume")
            .unwrap()
            .description
            .clone()
            .unwrap_or_default();
        assert!(
            create_volume_description.contains("BILLABLE"),
            "create_volume description must warn BILLABLE, got: {create_volume_description}"
        );
    }
}
