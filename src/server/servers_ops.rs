//! Server mutations and metrics, global actions, pricing (M15a, brief B23); this skeleton freezes the router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = servers_ops_router, vis = "pub(crate)")]
impl HcloudServer {}
