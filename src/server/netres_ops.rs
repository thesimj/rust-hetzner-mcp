//! Network/firewall/floating_ip/primary_ip mutations (M15c, brief B25); this skeleton freezes the router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = netres_ops_router, vis = "pub(crate)")]
impl HcloudServer {}
