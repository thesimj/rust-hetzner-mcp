//! Load balancer and DNS zone (+rrset) mutations (M15d, brief B26); this skeleton freezes the router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = lb_zone_ops_router, vis = "pub(crate)")]
impl HcloudServer {}
