use anyhow::Result;
use rmcp::{ServiceExt, transport::io};
use tracing_subscriber::{EnvFilter, fmt};

mod connection;
mod server;

use server::GodotMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    // All logging goes to stderr (stdout is the MCP transport)
    fmt::Subscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Starting Godot MCP server...");

    let server = GodotMcpServer::new()?;
    server.try_connect_editor().await;

    tracing::info!(
        "Godot MCP server running on stdio (editor: {})",
        if server.is_editor_connected() {
            "connected"
        } else {
            "not connected"
        }
    );

    let service = server.serve(io::stdio()).await?;
    service.waiting().await?;

    Ok(())
}
