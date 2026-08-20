//! Compute tools: servers, images, server_types, ssh_keys. Implemented in
//! milestone M3a (brief B3); this skeleton freezes the router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = compute_router, vis = "pub(crate)")]
impl HcloudServer {}
