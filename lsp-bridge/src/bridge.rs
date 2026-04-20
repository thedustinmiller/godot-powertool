use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::AtomicU64,
    },
};

use anyhow::Result;
use tokio::{
    io::{self, BufReader},
    sync::{Mutex, mpsc, oneshot},
};

use crate::composer;
use powertool_common::lsp::{connection, framing};

/// Tracks open file URIs so the composer knows which files to fan out over.
pub type OpenFiles = Arc<Mutex<HashSet<String>>>;

/// Pending composer sub-requests awaiting Godot responses.
pub type ComposerPending = Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>;

/// Run the bridge. Starts reading stdin immediately and connects TCP in the background.
/// Messages from stdin are buffered until TCP is connected.
/// Returns Ok(()) when stdin closes (Claude Code exited).
pub async fn run(host: String, start_port: u16) -> Result<()> {
    let (to_tcp_tx, to_tcp_rx) = mpsc::channel::<serde_json::Value>(64);
    let (to_stdout_tx, to_stdout_rx) = mpsc::channel::<serde_json::Value>(64);

    let open_files: OpenFiles = Arc::new(Mutex::new(HashSet::new()));
    let composer_pending: ComposerPending = Arc::new(Mutex::new(HashMap::new()));
    let comp_id = Arc::new(AtomicU64::new(0));

    // Spawn stdout writer — always active
    let stdout_handle = tokio::spawn(stdout_writer_loop(to_stdout_rx));

    // Spawn stdin reader — always active, buffers via channel
    let stdin_handle = tokio::spawn(stdin_loop(
        to_tcp_tx.clone(),
        to_stdout_tx.clone(),
        open_files,
        composer_pending.clone(),
        comp_id,
    ));

    // TCP connection loop — connects, proxies, reconnects on drop
    let tcp_handle = tokio::spawn(tcp_loop(
        host,
        start_port,
        to_tcp_rx,
        to_stdout_tx,
        composer_pending,
    ));

    // Wait for stdin to close (Claude Code exited) — that's our shutdown signal
    let _ = stdin_handle.await;

    // Clean up
    tcp_handle.abort();
    stdout_handle.abort();

    Ok(())
}

/// Connect to TCP, proxy messages, reconnect on disconnect. Runs forever.
async fn tcp_loop(
    host: String,
    start_port: u16,
    mut to_tcp_rx: mpsc::Receiver<serde_json::Value>,
    to_stdout: mpsc::Sender<serde_json::Value>,
    composer_pending: ComposerPending,
) {
    loop {
        // Connect (with backoff)
        let (tcp, port) = match connection::connect_with_backoff(&host, start_port).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to connect to Godot LSP: {e}");
                return;
            }
        };
        tracing::info!("Connected to Godot LSP on port {port}");

        let (tcp_read, tcp_write) = io::split(tcp);
        let mut tcp_reader = BufReader::new(tcp_read);
        let tcp_writer = Arc::new(Mutex::new(tcp_write));

        // Proxy until TCP drops
        let tcp_writer_clone = tcp_writer.clone();
        let pending = composer_pending.clone();
        let stdout = to_stdout.clone();

        tokio::select! {
            // Write messages from stdin → TCP
            _ = async {
                loop {
                    match to_tcp_rx.recv().await {
                        Some(msg) => {
                            let mut writer = tcp_writer_clone.lock().await;
                            if let Err(e) = framing::write_message(&mut *writer, &msg).await {
                                tracing::warn!("TCP write failed: {e}");
                                return;
                            }
                        }
                        None => return, // stdin closed
                    }
                }
            } => {}
            // Read messages from TCP → stdout
            _ = async {
                loop {
                    match framing::read_message(&mut tcp_reader).await {
                        Ok(Some(msg)) => {
                            let is_composer = msg
                                .get("id")
                                .and_then(|v| v.as_str())
                                .is_some_and(|id| id.starts_with("__comp_"));

                            if is_composer {
                                let id = msg["id"].as_str().unwrap().to_string();
                                let mut p = pending.lock().await;
                                if let Some(tx) = p.remove(&id) {
                                    let _ = tx.send(msg);
                                } else {
                                    tracing::warn!("orphan composer response for {id}");
                                }
                            } else {
                                let _ = stdout.send(msg).await;
                            }
                        }
                        Ok(None) => {
                            tracing::warn!("Godot LSP closed connection");
                            return;
                        }
                        Err(e) => {
                            tracing::warn!("TCP read error: {e}");
                            return;
                        }
                    }
                }
            } => {}
        }

        // TCP dropped — reconnect after a short delay
        tracing::warn!("Godot LSP disconnected, reconnecting in 1s...");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

/// Read LSP messages from stdin and route them.
async fn stdin_loop(
    to_tcp: mpsc::Sender<serde_json::Value>,
    to_stdout: mpsc::Sender<serde_json::Value>,
    open_files: OpenFiles,
    composer_pending: ComposerPending,
    comp_id: Arc<AtomicU64>,
) {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);

    loop {
        let msg = match framing::read_message(&mut reader).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                tracing::info!("stdin closed");
                return;
            }
            Err(e) => {
                tracing::warn!("stdin read error: {e}");
                return;
            }
        };

        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");

        match method {
            "textDocument/didOpen" => {
                if let Some(uri) = extract_text_document_uri(&msg) {
                    open_files.lock().await.insert(uri);
                }
                let _ = to_tcp.send(msg).await;
            }
            "textDocument/didClose" => {
                if let Some(uri) = extract_text_document_uri(&msg) {
                    open_files.lock().await.remove(&uri);
                }
                let _ = to_tcp.send(msg).await;
            }
            "workspace/symbol" => {
                let original_id = msg.get("id").cloned().unwrap_or_default();
                let params = msg.get("params").cloned().unwrap_or_default();
                let to_tcp = to_tcp.clone();
                let to_stdout = to_stdout.clone();
                let open_files = open_files.clone();
                let pending = composer_pending.clone();
                let cid = comp_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = composer::compose_workspace_symbol(
                        original_id, params, &open_files, &to_tcp, &to_stdout, &pending, &cid,
                    ).await {
                        tracing::warn!("workspace/symbol composition failed: {e}");
                    }
                });
            }
            "textDocument/prepareCallHierarchy" => {
                let original_id = msg.get("id").cloned().unwrap_or_default();
                let params = msg.get("params").cloned().unwrap_or_default();
                let to_tcp = to_tcp.clone();
                let to_stdout = to_stdout.clone();
                let pending = composer_pending.clone();
                let cid = comp_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = composer::compose_prepare_call_hierarchy(
                        original_id, params, &to_tcp, &to_stdout, &pending, &cid,
                    ).await {
                        tracing::warn!("prepareCallHierarchy composition failed: {e}");
                    }
                });
            }
            "callHierarchy/incomingCalls" => {
                let original_id = msg.get("id").cloned().unwrap_or_default();
                let params = msg.get("params").cloned().unwrap_or_default();
                let to_tcp = to_tcp.clone();
                let to_stdout = to_stdout.clone();
                let pending = composer_pending.clone();
                let cid = comp_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = composer::compose_incoming_calls(
                        original_id, params, &to_tcp, &to_stdout, &pending, &cid,
                    ).await {
                        tracing::warn!("incomingCalls composition failed: {e}");
                    }
                });
            }
            "callHierarchy/outgoingCalls" => {
                let original_id = msg.get("id").cloned().unwrap_or_default();
                let params = msg.get("params").cloned().unwrap_or_default();
                let to_tcp = to_tcp.clone();
                let to_stdout = to_stdout.clone();
                let pending = composer_pending.clone();
                let cid = comp_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = composer::compose_outgoing_calls(
                        original_id, params, &to_tcp, &to_stdout, &pending, &cid,
                    ).await {
                        tracing::warn!("outgoingCalls composition failed: {e}");
                    }
                });
            }
            _ => {
                let _ = to_tcp.send(msg).await;
            }
        }
    }
}

/// Write messages from the channel to stdout.
async fn stdout_writer_loop(mut rx: mpsc::Receiver<serde_json::Value>) {
    let mut stdout = io::stdout();
    while let Some(msg) = rx.recv().await {
        if let Err(e) = framing::write_message(&mut stdout, &msg).await {
            tracing::warn!("stdout write failed: {e}");
            return;
        }
    }
}

/// Extract `params.textDocument.uri` from an LSP message.
fn extract_text_document_uri(msg: &serde_json::Value) -> Option<String> {
    msg.get("params")?
        .get("textDocument")?
        .get("uri")?
        .as_str()
        .map(String::from)
}
