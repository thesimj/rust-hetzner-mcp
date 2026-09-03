//! Composition root: argv -> config file -> stdio MCP server. stdout is the
//! MCP wire, so only `--help`/`--version` print there; errors go to stderr
//! via anyhow (`Error: ...` plus the `Caused by:` chain) with exit code 1.

use hetzner_mcp::{cli, config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match cli::parse(std::env::args_os().skip(1))? {
        cli::Cli::Help => {
            let shown = config::default_path()
                .and_then(|p| Ok(std::path::absolute(p)?))
                .map_or_else(|e| e.to_string(), |p| p.display().to_string());
            println!("{}", cli::help_text(&shown));
            Ok(())
        }
        cli::Cli::Version => {
            println!("hetzner-mcp {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        cli::Cli::Serve { config } => {
            let config = config::load(config.as_deref())?;
            hetzner_mcp::server::run(config).await
        }
    }
}
