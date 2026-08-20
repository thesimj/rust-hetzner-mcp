//! Infra tools: locations, datacenters, volumes, networks, firewalls.
//! Implemented in milestone M3b (brief B4); this skeleton freezes the router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = infra_router, vis = "pub(crate)")]
impl HcloudServer {}
