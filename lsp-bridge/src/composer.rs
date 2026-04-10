use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Result, bail};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use crate::bridge::{ComposerPending, OpenFiles};

const SUB_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Send a sub-request to Godot and await the response.
async fn sub_request(
    to_tcp: &mpsc::Sender<Value>,
    pending: &ComposerPending,
    comp_id: &AtomicU64,
    method: &str,
    params: Value,
) -> Result<Value> {
    let n = comp_id.fetch_add(1, Ordering::Relaxed);
    let id = format!("__comp_{n}");

    let request = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });

    let (tx, rx) = oneshot::channel();
    pending.lock().await.insert(id.clone(), tx);

    to_tcp
        .send(request)
        .await
        .map_err(|_| anyhow::anyhow!("TCP channel closed"))?;

    match tokio::time::timeout(SUB_REQUEST_TIMEOUT, rx).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(_)) => {
            bail!("sub-request channel closed (Godot disconnected)")
        }
        Err(_) => {
            pending.lock().await.remove(&id);
            bail!("sub-request '{method}' timed out after {SUB_REQUEST_TIMEOUT:?}")
        }
    }
}

/// Extract the `result` field from an LSP response, defaulting to empty array.
fn result_or_empty(response: &Value) -> Value {
    response
        .get("result")
        .cloned()
        .unwrap_or(Value::Array(vec![]))
}

// ─── workspace/symbol ───────────────────────────────────────────────

/// Compose `workspace/symbol` from `textDocument/documentSymbol` across all open files.
pub async fn compose_workspace_symbol(
    original_id: Value,
    params: Value,
    open_files: &OpenFiles,
    to_tcp: &mpsc::Sender<Value>,
    to_stdout: &mpsc::Sender<Value>,
    pending: &ComposerPending,
    comp_id: &AtomicU64,
) -> Result<()> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();

    let uris: Vec<String> = open_files.lock().await.iter().cloned().collect();

    if uris.is_empty() {
        send_response(to_stdout, original_id, json!([])).await?;
        return Ok(());
    }

    // Fan out documentSymbol requests
    let mut results = Vec::new();
    for uri in &uris {
        let response = sub_request(
            to_tcp,
            pending,
            comp_id,
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await?;

        let symbols = result_or_empty(&response);
        if let Value::Array(syms) = symbols {
            flatten_symbols(&syms, uri, &mut results);
        }
    }

    // Filter by query if non-empty
    if !query.is_empty() {
        results.retain(|sym| {
            sym.get("name")
                .and_then(|v| v.as_str())
                .is_some_and(|name| name.to_lowercase().contains(&query))
        });
    }

    send_response(to_stdout, original_id, Value::Array(results)).await
}

/// Recursively flatten `DocumentSymbol[]` into `SymbolInformation[]`.
fn flatten_symbols(symbols: &[Value], uri: &str, out: &mut Vec<Value>) {
    for sym in symbols {
        let range = sym.get("range").cloned().unwrap_or_default();
        let selection_range = sym.get("selectionRange").cloned().unwrap_or(range.clone());

        out.push(json!({
            "name": sym.get("name").cloned().unwrap_or_default(),
            "kind": sym.get("kind").cloned().unwrap_or(json!(1)),
            "location": {
                "uri": uri,
                "range": selection_range,
            },
            "containerName": sym.get("detail").cloned().unwrap_or(Value::Null),
        }));

        if let Some(Value::Array(children)) = sym.get("children") {
            flatten_symbols(children, uri, out);
        }
    }
}

// ─── textDocument/prepareCallHierarchy ──────────────────────────────

/// Compose `prepareCallHierarchy` from `textDocument/documentSymbol`.
pub async fn compose_prepare_call_hierarchy(
    original_id: Value,
    params: Value,
    to_tcp: &mpsc::Sender<Value>,
    to_stdout: &mpsc::Sender<Value>,
    pending: &ComposerPending,
    comp_id: &AtomicU64,
) -> Result<()> {
    let uri = params
        .pointer("/textDocument/uri")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let position = params.get("position").cloned().unwrap_or_default();

    let response = sub_request(
        to_tcp,
        pending,
        comp_id,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri } }),
    )
    .await?;

    let symbols = result_or_empty(&response);
    let item = if let Value::Array(syms) = &symbols {
        find_innermost_symbol(syms, &position).map(|sym| {
            json!({
                "name": sym.get("name").cloned().unwrap_or_default(),
                "kind": sym.get("kind").cloned().unwrap_or(json!(12)),
                "uri": uri,
                "range": sym.get("range").cloned().unwrap_or_default(),
                "selectionRange": sym.get("selectionRange").cloned().unwrap_or_default(),
            })
        })
    } else {
        None
    };

    let result = match item {
        Some(i) => json!([i]),
        None => json!([]),
    };

    send_response(to_stdout, original_id, result).await
}

/// Find the innermost (most deeply nested) symbol whose range contains `position`.
fn find_innermost_symbol<'a>(symbols: &'a [Value], position: &Value) -> Option<&'a Value> {
    let line = position.get("line")?.as_u64()?;
    let character = position.get("character")?.as_u64()?;

    let mut best: Option<&Value> = None;

    for sym in symbols {
        let range = sym.get("range")?;
        if !range_contains(range, line, character) {
            continue;
        }

        // Check children for a tighter match
        if let Some(Value::Array(children)) = sym.get("children") {
            if let Some(child) = find_innermost_symbol(children, position) {
                best = Some(child);
                continue;
            }
        }

        best = Some(sym);
    }

    best
}

/// Check if an LSP Range contains a position.
fn range_contains(range: &Value, line: u64, character: u64) -> bool {
    let Some(start) = range.get("start") else {
        return false;
    };
    let Some(end) = range.get("end") else {
        return false;
    };

    let start_line = start.get("line").and_then(|v| v.as_u64()).unwrap_or(0);
    let start_char = start
        .get("character")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let end_line = end.get("line").and_then(|v| v.as_u64()).unwrap_or(0);
    let end_char = end
        .get("character")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if line < start_line || line > end_line {
        return false;
    }
    if line == start_line && character < start_char {
        return false;
    }
    if line == end_line && character > end_char {
        return false;
    }
    true
}

// ─── callHierarchy/incomingCalls ────────────────────────────────────

/// Compose `incomingCalls` from `textDocument/references`.
pub async fn compose_incoming_calls(
    original_id: Value,
    params: Value,
    to_tcp: &mpsc::Sender<Value>,
    to_stdout: &mpsc::Sender<Value>,
    pending: &ComposerPending,
    comp_id: &AtomicU64,
) -> Result<()> {
    let item = params.get("item").cloned().unwrap_or_default();
    let uri = item.get("uri").and_then(|v| v.as_str()).unwrap_or("");
    let selection_range = item.get("selectionRange").cloned().unwrap_or_default();

    // Get the start position of the symbol for references lookup
    let position = selection_range
        .get("start")
        .cloned()
        .unwrap_or(json!({"line": 0, "character": 0}));

    let response = sub_request(
        to_tcp,
        pending,
        comp_id,
        "textDocument/references",
        json!({
            "textDocument": { "uri": uri },
            "position": position,
            "context": { "includeDeclaration": false },
        }),
    )
    .await?;

    let locations = result_or_empty(&response);
    let mut calls = Vec::new();

    if let Value::Array(locs) = locations {
        for loc in &locs {
            let ref_uri = loc.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let ref_range = loc.get("range").cloned().unwrap_or_default();

            // Build a minimal CallHierarchyItem for the caller
            // Name is derived from the file — a full implementation would
            // do a documentSymbol lookup to find the enclosing function.
            let caller_name = ref_uri.rsplit('/').next().unwrap_or("unknown");

            calls.push(json!({
                "from": {
                    "name": caller_name,
                    "kind": 12, // Function
                    "uri": ref_uri,
                    "range": ref_range,
                    "selectionRange": ref_range,
                },
                "fromRanges": [ref_range],
            }));
        }
    }

    send_response(to_stdout, original_id, Value::Array(calls)).await
}

// ─── callHierarchy/outgoingCalls ────────────────────────────────────

/// Compose `outgoingCalls` from `textDocument/documentSymbol`.
/// Note: this is approximate — returns symbols defined within the caller's range,
/// not true call sites.
pub async fn compose_outgoing_calls(
    original_id: Value,
    params: Value,
    to_tcp: &mpsc::Sender<Value>,
    to_stdout: &mpsc::Sender<Value>,
    pending: &ComposerPending,
    comp_id: &AtomicU64,
) -> Result<()> {
    let item = params.get("item").cloned().unwrap_or_default();
    let uri = item.get("uri").and_then(|v| v.as_str()).unwrap_or("");
    let caller_range = item.get("range").cloned().unwrap_or_default();

    let response = sub_request(
        to_tcp,
        pending,
        comp_id,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": uri } }),
    )
    .await?;

    let symbols = result_or_empty(&response);
    let mut calls = Vec::new();

    if let Value::Array(syms) = symbols {
        collect_symbols_in_range(&syms, &caller_range, uri, &mut calls);
    }

    send_response(to_stdout, original_id, Value::Array(calls)).await
}

/// Collect symbols whose range falls within `parent_range` (children of the caller).
fn collect_symbols_in_range(
    symbols: &[Value],
    parent_range: &Value,
    uri: &str,
    out: &mut Vec<Value>,
) {
    let p_start_line = parent_range
        .pointer("/start/line")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let p_end_line = parent_range
        .pointer("/end/line")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    for sym in symbols {
        let range = match sym.get("range") {
            Some(r) => r,
            None => continue,
        };

        let sym_start = range
            .pointer("/start/line")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let sym_end = range
            .pointer("/end/line")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // Symbol must be strictly inside parent range
        if sym_start > p_start_line && sym_end < p_end_line {
            let sel_range = sym.get("selectionRange").cloned().unwrap_or(range.clone());
            out.push(json!({
                "to": {
                    "name": sym.get("name").cloned().unwrap_or_default(),
                    "kind": sym.get("kind").cloned().unwrap_or(json!(12)),
                    "uri": uri,
                    "range": range,
                    "selectionRange": sel_range,
                },
                "fromRanges": [range],
            }));
        }

        // Recurse into children
        if let Some(Value::Array(children)) = sym.get("children") {
            collect_symbols_in_range(children, parent_range, uri, out);
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────

/// Send a JSON-RPC response to stdout via the channel.
async fn send_response(
    to_stdout: &mpsc::Sender<Value>,
    id: Value,
    result: Value,
) -> Result<()> {
    let response = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });

    to_stdout
        .send(response)
        .await
        .map_err(|_| anyhow::anyhow!("stdout channel closed"))?;

    Ok(())
}
