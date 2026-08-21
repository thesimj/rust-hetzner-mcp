//! Image/ssh_key/volume/placement_group/certificate mutations (M15b, brief B24).
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

use super::{HcloudServer, respond};

const IMAGE_ACTIONS: [&str; 1] = ["change_protection"];
const VOLUME_ACTIONS: [&str; 4] = ["attach", "detach", "resize", "change_protection"];
const CERTIFICATE_ACTIONS: [&str; 1] = ["retry"];

/// Reject an action name not in the tool's allowlist before it reaches the
/// URL, mirroring compute.rs's `POWER_ACTIONS` check.
fn check_action(action: &str, allowed: &[&str]) -> Result<(), ErrorData> {
    if allowed.contains(&action) {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "action must be one of {}, got {:?}",
                allowed.join(", "),
                action
            ),
            None,
        ))
    }
}

/// Numeric ID of the resource to delete.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IdArgs {
    /// Numeric ID of the resource, from the matching list_*/get_* tool's response.
    pub id: u64,
}

/// Action name plus optional action-specific parameters, shared by every
/// `*_action` tool - the allowed action set differs per tool, so validation
/// stays in each tool body rather than on this shape.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ActionArgs {
    /// Numeric ID of the resource to act on.
    pub id: u64,
    /// Action name; see the tool description for the allowed set.
    pub action: String,
    /// Action-specific parameters, e.g. `{"delete": true}` for change_protection.
    pub params: Option<serde_json::Value>,
}

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
    /// Destination image type to convert to.
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
    /// Placement group type, e.g. "spread".
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
        description = "Update an image's description, type, or labels.",
        annotations(
            title = "Update image",
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
        description = "Run an image action (change_protection).",
        annotations(
            title = "Run image action",
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = true
        )
    )]
    pub(crate) async fn image_action(
        &self,
        Parameters(args): Parameters<ActionArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        check_action(&args.action, &IMAGE_ACTIONS)?;
        respond(
            self.client
                .post(
                    &format!("/images/{}/actions/{}", args.id, args.action),
                    args.params.unwrap_or_else(|| serde_json::json!({})),
                )
                .await,
        )
    }

    #[tool(
        description = "Update an SSH key's name or labels.",
        annotations(
            title = "Update SSH key",
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
        respond(self.client.put(&format!("/ssh_keys/{id}"), body).await)
    }

    #[tool(
        description = "Create a new block storage volume. This creates a BILLABLE resource.",
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
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/volumes", body).await)
    }

    #[tool(
        description = "Update a volume's name or labels.",
        annotations(
            title = "Update volume",
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
        description = "Run a volume action (attach, detach, resize, change_protection). \
        resize can only increase the size and cannot be undone.",
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
        check_action(&args.action, &VOLUME_ACTIONS)?;
        respond(
            self.client
                .post(
                    &format!("/volumes/{}/actions/{}", args.id, args.action),
                    args.params.unwrap_or_else(|| serde_json::json!({})),
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
        description = "Update a placement group's name or labels.",
        annotations(
            title = "Update placement group",
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
        description = "Upload a certificate, or request a managed Let's Encrypt certificate.",
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
        let body = serde_json::to_value(&args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        respond(self.client.post("/certificates", body).await)
    }

    #[tool(
        description = "Update a certificate's name or labels.",
        annotations(
            title = "Update certificate",
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
        description = "Run a certificate action (retry). Retries issuance of a managed \
        certificate that failed to be issued.",
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
        check_action(&args.action, &CERTIFICATE_ACTIONS)?;
        respond(
            self.client
                .post(
                    &format!("/certificates/{}/actions/{}", args.id, args.action),
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
    async fn update_image_sends_only_the_set_fields() {
        let mock = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/images/5"))
            .and(body_json(serde_json::json!({"description": "renamed"})))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"image": {"id": 5}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .update_image(Parameters(UpdateImageArgs {
                id: 5,
                description: Some("renamed".into()),
                r#type: None,
                labels: None,
            }))
            .await
            .unwrap();
        assert_eq!(
            tool_result_json(&res),
            serde_json::json!({"image": {"id": 5}})
        );
    }

    #[tokio::test]
    async fn delete_image_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/images/5"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_image(Parameters(IdArgs { id: 5 }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"success": true}));
    }

    #[tokio::test]
    async fn image_action_posts_the_action_path_with_params() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/5/actions/change_protection"))
            .and(body_json(serde_json::json!({"delete": true})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .image_action(Parameters(ActionArgs {
                id: 5,
                action: "change_protection".into(),
                params: Some(serde_json::json!({"delete": true})),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn image_action_rejects_unknown_actions_with_invalid_params() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .image_action(Parameters(ActionArgs {
                id: 5,
                action: "nuke".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn create_volume_sends_exactly_the_required_fields_when_optionals_are_unset() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/volumes"))
            .and(body_json(serde_json::json!({"size": 20, "name": "data-1"})))
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
                server: None,
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
    async fn delete_volume_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/volumes/3"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_volume(Parameters(IdArgs { id: 3 }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"success": true}));
    }

    #[tokio::test]
    async fn volume_action_posts_the_action_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/volumes/3/actions/resize"))
            .and(body_json(serde_json::json!({"size": 100})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .volume_action(Parameters(ActionArgs {
                id: 3,
                action: "resize".into(),
                params: Some(serde_json::json!({"size": 100})),
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn volume_action_rejects_unknown_actions_with_invalid_params() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .volume_action(Parameters(ActionArgs {
                id: 3,
                action: "nuke".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
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
    async fn delete_placement_group_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/placement_groups/4"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_placement_group(Parameters(IdArgs { id: 4 }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"success": true}));
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

    #[tokio::test]
    async fn delete_certificate_hits_the_id_path() {
        let mock = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/certificates/6"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .delete_certificate(Parameters(IdArgs { id: 6 }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"success": true}));
    }

    #[tokio::test]
    async fn certificate_action_posts_the_action_path() {
        let mock = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/certificates/6/actions/retry"))
            .and(body_json(serde_json::json!({})))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"action": {}})),
            )
            .mount(&mock)
            .await;

        let server = server_for(mock.uri());
        let res = server
            .certificate_action(Parameters(ActionArgs {
                id: 6,
                action: "retry".into(),
                params: None,
            }))
            .await
            .unwrap();
        assert_eq!(tool_result_json(&res), serde_json::json!({"action": {}}));
    }

    #[tokio::test]
    async fn certificate_action_rejects_unknown_actions_with_invalid_params() {
        let server = server_for("http://127.0.0.1:9".to_string());
        let err = server
            .certificate_action(Parameters(ActionArgs {
                id: 6,
                action: "nuke".into(),
                params: None,
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    /// Table-driven: the four `{id, name?, labels?}` update tools share
    /// `UpdateNameLabelsArgs` - one row per tool proves each hits its own
    /// path with an id-free body, since the shared struct makes a copy/paste
    /// path bug (e.g. update_volume PUTing to /ssh_keys/{id}) easy to miss.
    #[tokio::test]
    async fn update_name_labels_tools_hit_their_own_path_with_an_id_free_body() {
        let mock = MockServer::start().await;
        for path_str in [
            "/ssh_keys/10",
            "/volumes/10",
            "/placement_groups/10",
            "/certificates/10",
        ] {
            Mock::given(method("PUT"))
                .and(path(path_str))
                .and(body_json(serde_json::json!({"name": "renamed"})))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
                )
                .mount(&mock)
                .await;
        }

        let server = server_for(mock.uri());
        let args = || UpdateNameLabelsArgs {
            id: 10,
            name: Some("renamed".into()),
            labels: None,
        };
        for res in [
            server.update_ssh_key(Parameters(args())).await.unwrap(),
            server.update_volume(Parameters(args())).await.unwrap(),
            server
                .update_placement_group(Parameters(args()))
                .await
                .unwrap(),
            server.update_certificate(Parameters(args())).await.unwrap(),
        ] {
            assert_eq!(tool_result_json(&res), serde_json::json!({"ok": true}));
        }
    }

    #[test]
    fn res_ops_router_registers_all_15_tools_with_expected_annotations() {
        let router = super::HcloudServer::res_ops_router();
        let expected: [(&str, bool, bool); 15] = [
            ("update_image", false, false),
            ("delete_image", false, true),
            ("image_action", false, true),
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
