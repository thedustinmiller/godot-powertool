mod bridge;
mod composer;
mod connection;
mod framing;

use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Parser)]
#[command(name = "powertool-lsp-bridge")]
#[command(about = "Bridge between Claude Code (stdio) and Godot's GDScript LSP (TCP)")]
struct Cli {
    /// Godot LSP host
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Starting port to scan (scans port..port+10)
    #[arg(long, default_value_t = 6005)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Route all logging to stderr — stdout is the LSP transport
    fmt::Subscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    tracing::info!(
        "powertool-lsp-bridge starting, target {}:{}-{}",
        cli.host,
        cli.port,
        cli.port + 9
    );

    // Run the bridge — reads stdin immediately, connects TCP in background
    bridge::run(cli.host, cli.port).await
}
