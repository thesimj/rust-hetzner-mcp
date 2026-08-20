//! MCP server for the Hetzner Cloud API, plus the thin [`hcloud`] HTTP client
//! it is built on. The binary target (`hetzner-mcp`) runs [`server::run`].

pub mod hcloud;
pub mod server;
