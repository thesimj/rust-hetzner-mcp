//! MCP server for the Hetzner Cloud API, plus the thin [`hcloud`] HTTP client
//! it is built on. Credentials come from [`config`] (`config.toml`); the
//! binary target (`hetzner-mcp`) parses argv with [`cli`] and runs
//! [`server::run`].

pub mod cli;
pub mod config;
pub mod hcloud;
pub mod server;
