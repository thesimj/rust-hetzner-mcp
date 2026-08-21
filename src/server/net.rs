//! Network-adjacent tools: floating_ips, primary_ips, load_balancers,
//! load_balancer_types. Implemented in milestone M9a (brief B19); this
//! skeleton freezes the router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = net_router, vis = "pub(crate)")]
impl HcloudServer {}
