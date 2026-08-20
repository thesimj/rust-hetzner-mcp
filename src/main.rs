#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hetzner_mcp::server::run().await
}
