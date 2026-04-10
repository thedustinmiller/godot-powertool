use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tokio::net::TcpStream;

const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const PORT_RANGE: u16 = 10;
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const TOTAL_DEADLINE: Duration = Duration::from_secs(300);

/// Scan `start_port..start_port+10` for a listening Godot LSP server.
/// Returns the first successful connection (lowest port wins).
pub async fn probe_ports(host: &str, start_port: u16) -> Option<(TcpStream, u16)> {
    for port in start_port..start_port + PORT_RANGE {
        let addr = format!("{host}:{port}");
        match tokio::time::timeout(PROBE_TIMEOUT, TcpStream::connect(&addr)).await {
            Ok(Ok(stream)) => return Some((stream, port)),
            _ => continue,
        }
    }
    None
}

/// Connect to Godot's LSP with exponential backoff.
/// Scans ports on each attempt. Retries with 500ms→1s→2s→...→30s delays,
/// up to 300s total.
pub async fn connect_with_backoff(host: &str, start_port: u16) -> Result<(TcpStream, u16)> {
    // Try immediately first
    if let Some(result) = probe_ports(host, start_port).await {
        return Ok(result);
    }

    let deadline = Instant::now() + TOTAL_DEADLINE;
    let mut delay = Duration::from_millis(500);

    loop {
        if Instant::now() + delay > deadline {
            bail!(
                "Godot LSP not found on {host} ports {start_port}-{} after {}s",
                start_port + PORT_RANGE - 1,
                TOTAL_DEADLINE.as_secs()
            );
        }

        tracing::debug!(
            "Godot LSP not found, retrying in {:.1}s",
            delay.as_secs_f64()
        );
        tokio::time::sleep(delay).await;

        if let Some(result) = probe_ports(host, start_port).await {
            return Ok(result);
        }

        delay = (delay * 2).min(MAX_BACKOFF);
    }
}
