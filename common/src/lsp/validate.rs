use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::json;
use tokio::io::{self, BufReader};
use tokio::net::TcpStream;

use super::{connection, framing};

/// One LSP diagnostic, surfaced after a `textDocument/didOpen`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    pub uri: String,
    /// 1 = Error, 2 = Warning, 3 = Information, 4 = Hint
    pub severity: u32,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// One file to validate. `uri` should be a `file://` URI; `text` is the file content.
pub struct ScriptFile {
    pub uri: String,
    pub text: String,
}

/// Connect to Godot's LSP, open each script, collect diagnostics.
///
/// Per-file timeout: how long to wait for `publishDiagnostics` after `didOpen`.
/// The LSP doesn't send anything for a clean parse, so the timeout doubles as
/// the "no errors" signal.
pub async fn validate(
    host: &str,
    start_port: u16,
    files: Vec<ScriptFile>,
    per_file_timeout: Duration,
) -> Result<Vec<Diagnostic>> {
    let (stream, _port) = connection::probe_ports(host, start_port)
        .await
        .context("Godot LSP not reachable")?;
    validate_on(stream, files, per_file_timeout).await
}

async fn validate_on(
    stream: TcpStream,
    files: Vec<ScriptFile>,
    per_file_timeout: Duration,
) -> Result<Vec<Diagnostic>> {
    let (read, mut write) = io::split(stream);
    let mut reader = BufReader::new(read);

    // initialize
    framing::write_message(
        &mut write,
        &json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": std::process::id(),
                "rootUri": serde_json::Value::Null,
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": false }
                    }
                },
                "clientInfo": { "name": "powertool-validator" }
            }
        }),
    )
    .await?;

    // Drain until initialize response
    loop {
        let msg = framing::read_message(&mut reader)
            .await?
            .context("LSP closed during initialize")?;
        if msg.get("id").and_then(|v| v.as_i64()) == Some(1) {
            break;
        }
    }

    framing::write_message(
        &mut write,
        &json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        }),
    )
    .await?;

    let mut all_diags: Vec<Diagnostic> = Vec::new();

    for file in &files {
        framing::write_message(
            &mut write,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": file.uri,
                        "languageId": "gdscript",
                        "version": 1,
                        "text": file.text,
                    }
                }
            }),
        )
        .await?;

        // Wait up to the timeout for a publishDiagnostics matching this URI.
        // Godot's LSP sends one notification per didOpen, even when empty.
        let deadline = tokio::time::Instant::now() + per_file_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                tracing::debug!("LSP diagnostics timeout for {}", file.uri);
                break;
            }
            match tokio::time::timeout(remaining, framing::read_message(&mut reader)).await {
                Ok(Ok(Some(msg))) => {
                    if msg.get("method").and_then(|v| v.as_str())
                        == Some("textDocument/publishDiagnostics")
                    {
                        let p = msg.get("params").cloned().unwrap_or_default();
                        let uri = p
                            .get("uri")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if uri != file.uri {
                            continue;
                        }
                        if let Some(diags) = p.get("diagnostics").and_then(|v| v.as_array()) {
                            for d in diags {
                                let severity =
                                    d.get("severity").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
                                let range = d.get("range");
                                let start = range.and_then(|r| r.get("start"));
                                let line = start
                                    .and_then(|s| s.get("line"))
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                let column = start
                                    .and_then(|s| s.get("character"))
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                let message = d
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                all_diags.push(Diagnostic {
                                    uri: uri.clone(),
                                    severity,
                                    line,
                                    column,
                                    message,
                                });
                            }
                        }
                        break;
                    }
                }
                Ok(Ok(None)) => bail!("LSP closed mid-validation"),
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    tracing::debug!("LSP diagnostics timeout for {}", file.uri);
                    break;
                }
            }
        }
    }

    // Best-effort shutdown — don't fail the validation if these don't land.
    let _ = framing::write_message(
        &mut write,
        &json!({"jsonrpc": "2.0", "id": 999, "method": "shutdown"}),
    )
    .await;
    let _ = framing::write_message(
        &mut write,
        &json!({"jsonrpc": "2.0", "method": "exit"}),
    )
    .await;

    Ok(all_diags)
}

/// Filter to error-severity diagnostics (severity == 1).
pub fn errors_only(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags.iter().filter(|d| d.severity == 1).collect()
}
