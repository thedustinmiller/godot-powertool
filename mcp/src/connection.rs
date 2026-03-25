use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::tungstenite::Message;

/// Response from the Godot editor plugin.
#[derive(Debug)]
pub struct WsResponse {
    pub status: String,
    pub result: Option<serde_json::Value>,
    pub message: Option<String>,
    pub code: Option<String>,
}

type WsSink =
    futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>;

/// Manages the WebSocket connection to the Godot EditorPlugin.
pub struct EditorConnection {
    url: String,
    sender: Arc<Mutex<Option<WsSink>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<WsResponse>>>>,
    connected: Arc<AtomicBool>,
}

impl EditorConnection {
    pub fn new(port: u16) -> Self {
        Self {
            url: format!("ws://127.0.0.1:{port}"),
            sender: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Try to connect to the editor. Non-fatal — logs a warning on failure.
    pub async fn try_connect(&self) {
        if self.is_connected() {
            return;
        }
        if let Err(e) = self.connect().await {
            tracing::debug!("Editor not available at {}: {e}", self.url);
        }
    }

    /// Connect to the editor WebSocket server.
    pub async fn connect(&self) -> Result<()> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.url)
            .await
            .with_context(|| format!("Failed to connect to editor at {}", self.url))?;

        let (sink, stream) = ws_stream.split();
        *self.sender.lock().await = Some(sink);
        self.connected.store(true, Ordering::Relaxed);

        tracing::info!("Connected to Godot editor at {}", self.url);

        // Spawn background reader
        let pending = self.pending.clone();
        let connected = self.connected.clone();
        let sender = self.sender.clone();
        tokio::spawn(async move {
            Self::reader_loop(stream, pending, connected, sender).await;
        });

        Ok(())
    }

    /// Ensure we're connected, attempting connection if not.
    pub async fn ensure_connected(&self) -> Result<()> {
        if self.is_connected() {
            return Ok(());
        }
        self.connect().await
    }

    /// Send a command and await the response, with timeout.
    pub async fn send_command(
        &self,
        command: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<WsResponse> {
        self.ensure_connected().await?;

        let id = format!("cmd_{}", uuid::Uuid::new_v4());
        let request = serde_json::json!({
            "id": id,
            "command": command,
            "params": params,
        });

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), tx);

        // Send the message
        {
            let mut sender_guard = self.sender.lock().await;
            let sender = sender_guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("WebSocket not connected"))?;
            sender
                .send(Message::Text(serde_json::to_string(&request)?.into()))
                .await
                .context("Failed to send WebSocket message")?;
        }

        // Await response with timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                bail!("Response channel closed (editor disconnected)")
            }
            Err(_) => {
                // Remove pending entry on timeout
                self.pending.lock().await.remove(&id);
                bail!("Command '{command}' timed out after {:.0}s", timeout.as_secs_f64())
            }
        }
    }

    async fn reader_loop(
        mut stream: futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        pending: Arc<Mutex<HashMap<String, oneshot::Sender<WsResponse>>>>,
        connected: Arc<AtomicBool>,
        sender: Arc<Mutex<Option<WsSink>>>,
    ) {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        let id = json
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        let response = WsResponse {
                            status: json
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("error")
                                .to_string(),
                            result: json.get("result").cloned(),
                            message: json.get("message").and_then(|v| v.as_str()).map(String::from),
                            code: json.get("code").and_then(|v| v.as_str()).map(String::from),
                        };

                        if !id.is_empty() {
                            if let Some(tx) = pending.lock().await.remove(&id) {
                                let _ = tx.send(response);
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(e) => {
                    tracing::warn!("WebSocket error: {e}");
                    break;
                }
                _ => {}
            }
        }

        // Connection closed — mark disconnected and drain pending
        connected.store(false, Ordering::Relaxed);
        *sender.lock().await = None;

        let mut pending_guard = pending.lock().await;
        for (_, tx) in pending_guard.drain() {
            let _ = tx.send(WsResponse {
                status: "error".into(),
                result: None,
                message: Some("Editor connection closed".into()),
                code: Some("DISCONNECTED".into()),
            });
        }

        tracing::info!("Editor WebSocket disconnected");
    }
}
