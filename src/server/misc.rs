//! Remaining resource tools: certificates, isos, placement_groups, zones.
//! Implemented in milestone M9b (brief B20); this skeleton freezes the
//! router name.

use rmcp::tool_router;

use super::HcloudServer;

#[tool_router(router = misc_router, vis = "pub(crate)")]
impl HcloudServer {}
