//! Image/ssh_key/volume/placement_group/certificate mutations (M15b, brief B24); this skeleton freezes the router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = res_ops_router, vis = "pub(crate)")]
impl HcloudServer {}
