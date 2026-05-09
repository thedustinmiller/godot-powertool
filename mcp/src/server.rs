use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant},
};
use tokio::io::AsyncBufReadExt;

use anyhow::Result;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;

use powertool_common::{
    godot as godot_cli,
    lsp::validate as lsp_validate,
    platform::{find_godot_binary, godot_user_data_dir},
    project,
};

use crate::connection::EditorConnection;

/// Walk up from cwd looking for template.toml to find the project root.
fn find_project_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join("template.toml").exists() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// Bundled GDScript operations file
const GODOT_OPERATIONS_GD: &str = include_str!("../../scripts/godot_operations.gd");

/// Active Godot process state with live output capture.
struct GodotProcess {
    child: tokio::process::Child,
    output: Arc<Mutex<Vec<String>>>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl GodotProcess {
    /// Spawn Godot with stdout/stderr piped into shared buffers that can be read live.
    fn spawn(godot_path: &Path, args: &[String]) -> Result<Self, std::io::Error> {
        let mut child = tokio::process::Command::new(godot_path)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let output = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));

        if let Some(stdout) = child.stdout.take() {
            let buf = output.clone();
            tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    buf.lock().await.push(line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let buf = errors.clone();
            tokio::spawn(async move {
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    buf.lock().await.push(line);
                }
            });
        }

        Ok(GodotProcess { child, output, errors })
    }
}

const DEFAULT_EDITOR_PORT: u16 = 6550;
const DEFAULT_LSP_PORT: u16 = 6005;

pub struct GodotMcpServer {
    godot_path: PathBuf,
    canonical_godot_path: PathBuf,
    active_process: Arc<Mutex<Option<GodotProcess>>>,
    editor_process: Arc<Mutex<Option<GodotProcess>>>,
    operations_script: PathBuf,
    editor: Arc<EditorConnection>,
    /// Cache of (last_check, warning_text). Process enumeration is rerun at
    /// most once per `INSTANCE_CHECK_TTL` to keep tool-call overhead small.
    instance_check: StdMutex<Option<(Instant, Option<String>)>>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

const INSTANCE_CHECK_TTL: Duration = Duration::from_secs(2);

impl GodotMcpServer {
    pub fn new() -> Result<Self> {
        let project_root = find_project_root();
        if let Some(ref root) = project_root {
            tracing::info!("Project root: {}", root.display());
        }
        let godot_path = find_godot_binary(project_root.as_deref())?;
        tracing::info!("Using Godot at: {}", godot_path.display());

        // Write bundled operations script to temp directory
        let tmp_dir = std::env::temp_dir().join("godot-powertool-mcp");
        fs::create_dir_all(&tmp_dir)?;
        let operations_script = tmp_dir.join("godot_operations.gd");
        fs::write(&operations_script, GODOT_OPERATIONS_GD)?;

        let tool_router = Self::tool_router();

        // Editor WebSocket connection
        let port = std::env::var("POWERTOOL_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_EDITOR_PORT);
        let editor = Arc::new(EditorConnection::new(port));

        let canonical_godot_path = godot_path.canonicalize().unwrap_or_else(|_| godot_path.clone());

        Ok(Self {
            godot_path,
            canonical_godot_path,
            active_process: Arc::new(Mutex::new(None)),
            editor_process: Arc::new(Mutex::new(None)),
            operations_script,
            editor,
            instance_check: StdMutex::new(None),
            tool_router,
        })
    }

    fn validate_project_path(path: &str) -> Result<PathBuf, ErrorData> {
        if path.contains("..") {
            return Err(ErrorData::invalid_params(
                "Path traversal not allowed",
                None,
            ));
        }
        let p = PathBuf::from(path);
        let project_file = p.join("project.godot");
        if !project_file.exists() {
            return Err(ErrorData::invalid_params(
                format!("Not a valid Godot project: {path}"),
                None,
            ));
        }
        Ok(p)
    }

    async fn run_operation(
        &self,
        project_path: &Path,
        op: &str,
        params: &serde_json::Value,
        timeout: Duration,
    ) -> Result<String, ErrorData> {
        godot_cli::run_godot_operation_async(
            &self.godot_path,
            project_path,
            &self.operations_script,
            op,
            params,
            timeout,
        )
        .await
        .map_err(|e| ErrorData::internal_error(format!("Godot operation failed: {e}"), None))
    }

    fn timeout_from(timeout_seconds: Option<u64>) -> Duration {
        Duration::from_secs(timeout_seconds.unwrap_or(godot_cli::DEFAULT_TIMEOUT_SECS))
    }

    /// Try connecting to the editor on startup (non-fatal).
    pub async fn try_connect_editor(&self) {
        self.editor.try_connect().await;
    }

    pub fn is_editor_connected(&self) -> bool {
        self.editor.is_connected()
    }

    /// Send a command to the editor via WebSocket.
    async fn run_via_editor(
        &self,
        command: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, ErrorData> {
        let resp = self
            .editor
            .send_command(command, params, timeout)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        if resp.status == "success" {
            Ok(resp.result.unwrap_or(serde_json::Value::Null))
        } else {
            let code = resp.code.as_deref().unwrap_or("INTERNAL_ERROR");
            let msg = resp
                .message
                .unwrap_or_else(|| "Unknown editor error".into());

            // Build structured error data with details from the addon
            let error_data = resp.details.map(|d| serde_json::json!({
                "code": code,
                "details": d,
            }));

            match code {
                "INVALID_PARAMS" => Err(ErrorData::invalid_params(msg, error_data)),
                "NO_SCENE" => Err(ErrorData::invalid_params(msg, error_data)),
                _ => Err(ErrorData::internal_error(msg, error_data)),
            }
        }
    }

    /// Require an active editor connection, attempting to connect if needed.
    async fn require_editor(&self) -> Result<(), ErrorData> {
        if !self.editor.is_connected() {
            self.editor.ensure_connected().await.map_err(|_| {
                ErrorData::internal_error(
                    "Editor not connected. Launch the Godot editor with the PowerTool addon enabled.",
                    None,
                )
            })?;
        }
        Ok(())
    }

    /// Count Godot processes whose `exe()` matches this server's binary path.
    /// Returns 0 if process enumeration fails.
    fn count_godot_processes(&self) -> usize {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::new(),
        );
        sys.processes()
            .values()
            .filter(|p| match p.exe() {
                Some(exe) => {
                    exe == self.canonical_godot_path
                        || exe == self.godot_path
                        || exe
                            .canonicalize()
                            .map(|c| c == self.canonical_godot_path)
                            .unwrap_or(false)
                }
                None => false,
            })
            .count()
    }

    /// Returns a warning string when more than one Godot process is running
    /// against this MCP's binary. Cached for `INSTANCE_CHECK_TTL` to avoid
    /// scanning processes on every tool call.
    fn multi_instance_warning(&self) -> Option<String> {
        let now = Instant::now();
        if let Ok(guard) = self.instance_check.lock() {
            if let Some((ts, ref cached)) = *guard {
                if now.duration_since(ts) < INSTANCE_CHECK_TTL {
                    return cached.clone();
                }
            }
        }

        let count = self.count_godot_processes();
        let warning = if count > 1 {
            Some(format!(
                "⚠️ WARNING: {count} Godot processes match the binary this MCP server uses ({}). \
                 The MCP can only talk to one editor at a time, so changes may be applied to a \
                 different instance than expected. Stop extras with stop_editor or by closing \
                 them in the OS.",
                self.godot_path.display()
            ))
        } else {
            None
        };

        if let Ok(mut guard) = self.instance_check.lock() {
            *guard = Some((now, warning.clone()));
        }
        warning
    }

    /// Build a text-only success response, prepending the multi-instance
    /// warning when one is active. Used by every tool that returns text.
    fn text_response(&self, text: impl Into<String>) -> CallToolResult {
        let body = match self.multi_instance_warning() {
            Some(w) => format!("{w}\n\n{}", text.into()),
            None => text.into(),
        };
        CallToolResult::success(vec![Content::text(body)])
    }
}

// === Tool parameter types ===

#[derive(Debug, Deserialize, JsonSchema)]
struct ProjectPathParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RunSceneStandaloneParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Specific scene to run (optional, defaults to main scene)
    scene: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RunSceneParams {
    /// Scene path to run (e.g., "res://scenes/main.tscn"). If omitted, runs the project's main scene.
    scene: Option<String>,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListProjectsParams {
    /// Directory to search for Godot projects
    directory: String,
    /// Search recursively in subdirectories
    recursive: Option<bool>,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateSceneParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Path for the new scene file (relative to project)
    scene_path: String,
    /// Root node type (default: "Node2D")
    root_node_type: Option<String>,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AddNodeParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Path to the scene file
    scene_path: String,
    /// Type of node to add (e.g., "Sprite2D", "CollisionShape2D")
    node_type: String,
    /// Name for the new node
    node_name: String,
    /// Path to the parent node (default: root)
    parent_node_path: Option<String>,
    /// Properties to set on the node
    properties: Option<serde_json::Value>,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct LoadSpriteParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Path to the scene file
    scene_path: String,
    /// Path to the sprite node within the scene
    node_path: String,
    /// Path to the texture resource
    texture_path: String,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SaveSceneParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Path to the scene file to save
    scene_path: String,
    /// Optional new path to save as
    new_path: Option<String>,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExportMeshLibraryParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Path to the 3D scene file
    scene_path: String,
    /// Output path for the MeshLibrary resource
    output_path: String,
    /// Specific mesh item names to export (all if empty)
    mesh_item_names: Option<Vec<String>>,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FilePathParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Path to the file within the project
    file_path: String,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetGodotVersionParams {
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TakeScreenshotParams {
    /// File path to save screenshot to (optional, returns base64 if not set)
    file_path: Option<String>,
    /// Pause the game tree before capturing and resume after. Fixes timeouts on CPU-heavy scenes where _process() starves the debugger channel.
    pause_first: Option<bool>,
    /// Timeout in seconds for the operation (default: 15)
    timeout_seconds: Option<u64>,
}

// === Editor WebSocket tool parameter types ===

#[derive(Debug, Deserialize, JsonSchema)]
struct EditorNodeParams {
    /// Node path relative to the edited scene root. /root = scene root node, /root/Child = direct child. The scene root's own name is NOT part of the path (use /root, not /root/MySceneRoot).
    node_path: String,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateNodeParams {
    /// Parent node path relative to the edited scene root (default: "/root" = scene root). The scene root's own name is NOT part of the path.
    parent_path: Option<String>,
    /// Type of node to create (e.g., "Sprite2D", "CharacterBody2D")
    node_type: String,
    /// Name for the new node
    node_name: String,
    /// Initial properties to set on the node
    properties: Option<serde_json::Value>,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteNodeParams {
    /// Node path relative to the edited scene root (e.g., /root/Child). The scene root's own name is NOT part of the path.
    node_path: String,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UpdateNodePropertyParams {
    /// Node path relative to the edited scene root (e.g., /root/Child). The scene root's own name is NOT part of the path.
    node_path: String,
    /// Property name to update
    property: String,
    /// New value for the property
    value: serde_json::Value,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListNodesParams {
    /// Parent node path relative to the edited scene root (default: "/root" = scene root). The scene root's own name is NOT part of the path.
    parent_path: Option<String>,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditorScenePathParams {
    /// Scene path (e.g., "res://scenes/main.tscn")
    path: String,
    /// Root node type for scene creation (default: "Node2D")
    #[allow(dead_code)]
    root_type: Option<String>,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditorTimeoutParams {
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReloadSceneParams {
    /// Scene path to reload (e.g. "res://scenes/main.tscn"). If omitted,
    /// reloads the currently edited scene.
    path: Option<String>,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditorScriptParams {
    /// Path to the script file
    script_path: String,
    /// Script content
    content: Option<String>,
    /// Node path to attach script to (optional)
    node_path: Option<String>,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetScriptParams {
    /// Path to the script file (optional if node_path provided)
    script_path: Option<String>,
    /// Node to get script from (optional if script_path provided)
    node_path: Option<String>,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ExecuteEditorScriptParams {
    /// GDScript statements to execute in the editor context. Code runs inside a function body — only statements are valid (no extends, class_name, or func declarations). A 'scene' variable referencing the edited scene root is available. print() output is captured and returned.
    code: String,
    /// Timeout in seconds (default: 15)
    timeout_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetEditorLogParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Number of lines from the end to return (default: 50)
    tail_lines: Option<usize>,
}

// === Tool implementations ===

#[rmcp::tool_router]
impl GodotMcpServer {
    /// Get the installed Godot engine version
    #[rmcp::tool]
    async fn get_godot_version(
        &self,
        Parameters(params): Parameters<GetGodotVersionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let timeout = Self::timeout_from(params.timeout_seconds);
        let version = godot_cli::get_godot_version_async(&self.godot_path, timeout)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(self.text_response(version))
    }

    /// Launch the Godot editor for a project. Polls for WebSocket connection after spawning.
    #[rmcp::tool]
    async fn launch_editor(
        &self,
        Parameters(params): Parameters<ProjectPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let args = vec![
            "-e".to_string(),
            "--path".to_string(),
            project.to_string_lossy().to_string(),
        ];
        let proc = GodotProcess::spawn(&self.godot_path, &args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        {
            let mut editor = self.editor_process.lock().await;
            *editor = Some(proc);
        }

        // Poll for WebSocket connection — editor takes seconds to start its WS server
        let mut connected = false;
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.editor.try_connect().await;
            if self.editor.is_connected() {
                connected = true;
                break;
            }
        }

        let status = if connected { "connected" } else { "launched (not yet connected)" };
        Ok(self.text_response(format!(
            "Launched Godot editor for {} — {status}",
            params.project_path
        )))
    }

    /// Stop the Godot editor. Falls back to process search if the editor wasn't launched by this MCP instance.
    #[rmcp::tool]
    async fn stop_editor(&self) -> Result<CallToolResult, ErrorData> {
        let mut editor = self.editor_process.lock().await;
        match editor.take() {
            Some(mut proc) => {
                let _ = proc.child.kill().await;
                Ok(self.text_response(
                    "Editor stopped",
                ))
            }
            None => {
                // Fallback: find and kill Godot editor processes by binary path
                let godot_path = self.godot_path.to_string_lossy();
                let pattern = format!("{} -e", godot_path);
                let result = std::process::Command::new("pkill")
                    .args(["-f", &pattern])
                    .output();
                match result {
                    Ok(output) if output.status.success() => {
                        Ok(self.text_response(
                            "Editor stopped (found via process search)",
                        ))
                    }
                    _ => Ok(self.text_response(
                        "No editor process found",
                    )),
                }
            }
        }
    }

    /// Run a Godot scene as a standalone process outside the editor. Only use this if you specifically need to run outside the editor (e.g., for CI or testing without the editor). Prefer run_scene for normal use.
    #[rmcp::tool]
    async fn run_scene_standalone(
        &self,
        Parameters(params): Parameters<RunSceneStandaloneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;

        // Kill existing process
        {
            let mut active = self.active_process.lock().await;
            if let Some(mut proc) = active.take() {
                let _ = proc.child.kill().await;
            }
        }

        let project_str = project.to_string_lossy().to_string();
        let mut args = vec!["-d".to_string(), "--path".to_string(), project_str];
        if let Some(ref scene) = params.scene {
            args.push(scene.clone());
        }

        let proc = GodotProcess::spawn(&self.godot_path, &args)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        {
            let mut active = self.active_process.lock().await;
            *active = Some(proc);
        }

        let scene_info = params.scene.as_deref().unwrap_or("main scene");
        Ok(self.text_response(format!(
            "Running {scene_info} from {}",
            params.project_path
        )))
    }

    /// Get debug output (stdout/stderr) from Godot processes spawned by this MCP server.
    /// Combines output from `launch_editor` and `run_scene_standalone` if both are active.
    /// Note: requires the editor/scene to have been launched via this MCP — externally
    /// launched editors aren't captured. For those, use `get_editor_log`.
    #[rmcp::tool]
    async fn get_debug_output(&self) -> Result<CallToolResult, ErrorData> {
        let mut sources: Vec<serde_json::Value> = Vec::new();

        if let Some(proc) = &*self.editor_process.lock().await {
            sources.push(serde_json::json!({
                "source": "editor",
                "output": proc.output.lock().await.clone(),
                "errors": proc.errors.lock().await.clone(),
            }));
        }
        if let Some(proc) = &*self.active_process.lock().await {
            sources.push(serde_json::json!({
                "source": "standalone",
                "output": proc.output.lock().await.clone(),
                "errors": proc.errors.lock().await.clone(),
            }));
        }

        if sources.is_empty() {
            return Err(ErrorData::invalid_params(
                "No Godot processes captured by this MCP server. Launch via launch_editor or run_scene_standalone, or use get_editor_log to read the editor's log file.",
                None,
            ));
        }

        Ok(self.text_response(
            serde_json::to_string_pretty(&serde_json::json!({ "sources": sources })).unwrap_or_default(),
        ))
    }

    /// Read the Godot editor log file for a project. Useful for debugging errors that don't appear in get_debug_output (e.g., editor-mode crashes, scene loading failures).
    #[rmcp::tool]
    async fn get_editor_log(
        &self,
        Parameters(params): Parameters<GetEditorLogParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let project_godot = project.join("project.godot");
        let project_name = project::parse_project_name(&project_godot)
            .map_err(|e| ErrorData::internal_error(format!("Failed to read project name: {e}"), None))?;

        let log_path = godot_user_data_dir(&project_name).join("logs").join("godot.log");
        if !log_path.exists() {
            return Err(ErrorData::invalid_params(
                format!("Log file not found: {}", log_path.display()),
                None,
            ));
        }

        let content = fs::read_to_string(&log_path)
            .map_err(|e| ErrorData::internal_error(format!("Failed to read log: {e}"), None))?;

        let tail = params.tail_lines.unwrap_or(50);
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(tail);
        let result = lines[start..].join("\n");

        Ok(self.text_response(result))
    }

    /// Stop the standalone Godot scene process. Only needed if the scene was launched with run_scene_standalone.
    #[rmcp::tool]
    async fn stop_scene_standalone(&self) -> Result<CallToolResult, ErrorData> {
        let mut active = self.active_process.lock().await;
        match active.take() {
            Some(mut proc) => {
                let _ = proc.child.kill().await;
                let output = proc.output.lock().await.clone();
                let errors = proc.errors.lock().await.clone();
                let final_output = serde_json::json!({
                    "message": "Scene stopped",
                    "final_output": output,
                    "final_errors": errors,
                });
                Ok(self.text_response(
                    serde_json::to_string_pretty(&final_output).unwrap_or_default(),
                ))
            }
            None => Ok(self.text_response(
                "No scene was running",
            )),
        }
    }

    /// Find Godot projects in a directory
    #[rmcp::tool]
    async fn list_projects(
        &self,
        Parameters(params): Parameters<ListProjectsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let dir = PathBuf::from(&params.directory);
        if !dir.exists() {
            return Err(ErrorData::invalid_params(
                format!("Directory not found: {}", params.directory),
                None,
            ));
        }

        let timeout = Self::timeout_from(params.timeout_seconds);
        let recursive = params.recursive.unwrap_or(false);
        let dir_clone = dir.clone();
        let projects = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || project::find_projects(&dir_clone, recursive)),
        )
        .await
        .map_err(|_| ErrorData::internal_error("list_projects timed out", None))?
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let result: Vec<serde_json::Value> = projects
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "path": p.path.to_string_lossy(),
                })
            })
            .collect();

        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Get detailed information about a Godot project
    #[rmcp::tool]
    async fn get_project_info(
        &self,
        Parameters(params): Parameters<ProjectPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let project_clone = project.clone();
        let info = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || project::get_project_info(&project_clone)),
        )
        .await
        .map_err(|_| ErrorData::internal_error("get_project_info timed out", None))?
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let result = serde_json::json!({
            "name": info.name,
            "path": info.path.to_string_lossy(),
            "godot_version": info.godot_version,
            "scenes": info.scenes.iter().map(|s| s.to_string_lossy().to_string()).collect::<Vec<_>>(),
            "scripts": info.scripts.iter().map(|s| s.to_string_lossy().to_string()).collect::<Vec<_>>(),
        });

        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Create a new scene with a specified root node type
    #[rmcp::tool]
    async fn create_scene(
        &self,
        Parameters(params): Parameters<CreateSceneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let op_params = serde_json::json!({
            "scene_path": params.scene_path,
            "root_node_type": params.root_node_type.unwrap_or_else(|| "Node2D".to_string()),
        });
        let timeout = Self::timeout_from(params.timeout_seconds);
        let output = self.run_operation(&project, "create_scene", &op_params, timeout).await?;
        Ok(self.text_response(output))
    }

    /// Add a node to an existing scene
    #[rmcp::tool]
    async fn add_node(
        &self,
        Parameters(params): Parameters<AddNodeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let mut op_params = serde_json::json!({
            "scene_path": params.scene_path,
            "node_type": params.node_type,
            "node_name": params.node_name,
        });
        if let Some(ref parent) = params.parent_node_path {
            op_params["parent_node_path"] = serde_json::json!(parent);
        }
        if let Some(ref props) = params.properties {
            op_params["properties"] = props.clone();
        }
        let timeout = Self::timeout_from(params.timeout_seconds);
        let output = self.run_operation(&project, "add_node", &op_params, timeout).await?;
        Ok(self.text_response(output))
    }

    /// Load a texture into a sprite node in a scene
    #[rmcp::tool]
    async fn load_sprite(
        &self,
        Parameters(params): Parameters<LoadSpriteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let op_params = serde_json::json!({
            "scene_path": params.scene_path,
            "node_path": params.node_path,
            "texture_path": params.texture_path,
        });
        let timeout = Self::timeout_from(params.timeout_seconds);
        let output = self.run_operation(&project, "load_sprite", &op_params, timeout).await?;
        Ok(self.text_response(output))
    }

    /// Save changes to a scene file
    #[rmcp::tool]
    async fn save_scene(
        &self,
        Parameters(params): Parameters<SaveSceneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let mut op_params = serde_json::json!({
            "scene_path": params.scene_path,
        });
        if let Some(ref new) = params.new_path {
            op_params["new_path"] = serde_json::json!(new);
        }
        let timeout = Self::timeout_from(params.timeout_seconds);
        let output = self.run_operation(&project, "save_scene", &op_params, timeout).await?;
        Ok(self.text_response(output))
    }

    /// Export a 3D scene as a MeshLibrary resource
    #[rmcp::tool]
    async fn export_mesh_library(
        &self,
        Parameters(params): Parameters<ExportMeshLibraryParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let mut op_params = serde_json::json!({
            "scene_path": params.scene_path,
            "output_path": params.output_path,
        });
        if let Some(ref names) = params.mesh_item_names {
            op_params["mesh_item_names"] = serde_json::json!(names);
        }
        let timeout = Self::timeout_from(params.timeout_seconds);
        let output = self.run_operation(&project, "export_mesh_library", &op_params, timeout).await?;
        Ok(self.text_response(output))
    }

    /// Get the UID for a specific file (Godot 4.4+)
    #[rmcp::tool]
    async fn get_uid(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let op_params = serde_json::json!({
            "file_path": params.file_path,
        });
        let timeout = Self::timeout_from(params.timeout_seconds);
        let output = self.run_operation(&project, "get_uid", &op_params, timeout).await?;
        Ok(self.text_response(output))
    }

    /// Update UID references by resaving all resources (Godot 4.4+)
    #[rmcp::tool]
    async fn update_project_uids(
        &self,
        Parameters(params): Parameters<ProjectPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let op_params = serde_json::json!({
            "project_path": params.project_path,
        });
        let timeout = Self::timeout_from(params.timeout_seconds);
        let output = self.run_operation(&project, "resave_resources", &op_params, timeout).await?;
        Ok(self.text_response(output))
    }

    // === Editor WebSocket tools (require editor connection) ===

    /// Create a node in the live editor scene tree
    #[rmcp::tool]
    async fn create_node(
        &self,
        Parameters(params): Parameters<CreateNodeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let mut p = serde_json::json!({
            "parent_path": params.parent_path.unwrap_or_else(|| "/root".into()),
            "node_type": params.node_type,
            "node_name": params.node_name,
        });
        if let Some(ref props) = params.properties {
            p["properties"] = props.clone();
        }
        let result = self.run_via_editor("create_node", p, timeout).await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Delete a node from the live editor scene tree
    #[rmcp::tool]
    async fn delete_node(
        &self,
        Parameters(params): Parameters<DeleteNodeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor(
                "delete_node",
                serde_json::json!({"node_path": params.node_path}),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Update a property on a node in the live editor
    #[rmcp::tool]
    async fn update_node_property(
        &self,
        Parameters(params): Parameters<UpdateNodePropertyParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor(
                "update_node_property",
                serde_json::json!({
                    "node_path": params.node_path,
                    "property": params.property,
                    "value": params.value,
                }),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Get all properties of a node in the live editor
    #[rmcp::tool]
    async fn get_node_properties(
        &self,
        Parameters(params): Parameters<EditorNodeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor(
                "get_node_properties",
                serde_json::json!({"node_path": params.node_path}),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// List child nodes of a parent in the live editor scene tree
    #[rmcp::tool]
    async fn list_nodes(
        &self,
        Parameters(params): Parameters<ListNodesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor(
                "list_nodes",
                serde_json::json!({
                    "parent_path": params.parent_path.unwrap_or_else(|| "/root".into()),
                }),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Open a scene in the editor. **Close the scene with `close_scene` when
    /// you finish editing it** — leaving scenes open across unrelated tasks
    /// is the main source of editor/agent confusion. If you also edit the
    /// scene's `.tscn` file directly while it is open, call
    /// `reload_scene_from_disk` afterward so the editor's in-memory copy
    /// matches the file on disk (otherwise the editor's copy will overwrite
    /// your edits on the next save and a "reload from disk?" popup will
    /// block further work).
    #[rmcp::tool]
    async fn open_scene(
        &self,
        Parameters(params): Parameters<EditorScenePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor(
                "open_scene",
                serde_json::json!({"path": params.path}),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Reload an open scene from disk, discarding the editor's in-memory
    /// copy. Use this after writing to a `.tscn` file that is currently
    /// open in the editor — it suppresses the "scene was modified
    /// externally" popup and ensures the editor is in sync with disk.
    /// With no `path`, reloads the currently edited scene.
    #[rmcp::tool]
    async fn reload_scene_from_disk(
        &self,
        Parameters(params): Parameters<ReloadSceneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let mut payload = serde_json::Map::new();
        if let Some(p) = params.path {
            payload.insert("path".into(), serde_json::Value::String(p));
        }
        let result = self
            .run_via_editor(
                "reload_scene_from_disk",
                serde_json::Value::Object(payload),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Close the currently active scene tab in the editor. Call this when
    /// done editing a scene so the agent doesn't accidentally apply later
    /// changes to it. **Discards** unsaved in-editor changes — call
    /// `save_scene` first if needed.
    #[rmcp::tool]
    async fn close_scene(
        &self,
        Parameters(params): Parameters<EditorTimeoutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor("close_scene", serde_json::json!({}), timeout)
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Get info about the currently open scene in the editor
    #[rmcp::tool]
    async fn get_current_scene(
        &self,
        Parameters(params): Parameters<EditorTimeoutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor("get_current_scene", serde_json::json!({}), timeout)
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Get the full scene tree structure of the currently open scene
    #[rmcp::tool]
    async fn get_scene_structure(
        &self,
        Parameters(params): Parameters<EditorTimeoutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor("get_scene_structure", serde_json::json!({}), timeout)
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Create a new GDScript file via the editor
    #[rmcp::tool]
    async fn create_script_editor(
        &self,
        Parameters(params): Parameters<EditorScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let mut p = serde_json::json!({"script_path": params.script_path});
        if let Some(ref c) = params.content {
            p["content"] = serde_json::json!(c);
        }
        if let Some(ref n) = params.node_path {
            p["node_path"] = serde_json::json!(n);
        }
        let result = self.run_via_editor("create_script", p, timeout).await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Edit an existing GDScript file via the editor
    #[rmcp::tool]
    async fn edit_script_editor(
        &self,
        Parameters(params): Parameters<EditorScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor(
                "edit_script",
                serde_json::json!({
                    "script_path": params.script_path,
                    "content": params.content.unwrap_or_default(),
                }),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Get a script's content via the editor
    #[rmcp::tool]
    async fn get_script_editor(
        &self,
        Parameters(params): Parameters<GetScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let mut p = serde_json::json!({});
        if let Some(ref sp) = params.script_path {
            p["script_path"] = serde_json::json!(sp);
        }
        if let Some(ref np) = params.node_path {
            p["node_path"] = serde_json::json!(np);
        }
        let result = self.run_via_editor("get_script", p, timeout).await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Get the current editor state (open scene, selection, playing status)
    #[rmcp::tool]
    async fn get_editor_state(
        &self,
        Parameters(params): Parameters<EditorTimeoutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor("get_editor_state", serde_json::json!({}), timeout)
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Get the currently selected node in the editor
    #[rmcp::tool]
    async fn get_selected_node(
        &self,
        Parameters(params): Parameters<EditorTimeoutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor("get_selected_node", serde_json::json!({}), timeout)
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Execute arbitrary GDScript code in the editor context
    #[rmcp::tool]
    async fn execute_editor_script(
        &self,
        Parameters(params): Parameters<ExecuteEditorScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor(
                "execute_editor_script",
                serde_json::json!({"code": params.code}),
                timeout,
            )
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Run a scene via the Godot editor. Requires the editor to be running with the PowerTool addon. If no scene path is given, runs the project's main scene.
    /// Pre-validates GDScript syntax via the editor's LSP (port 6005 by default; override with POWERTOOL_LSP_PORT). Falls back to `--check-only` per script if the LSP is unreachable. Set POWERTOOL_SKIP_VALIDATION=1 to skip.
    #[rmcp::tool]
    async fn run_scene(
        &self,
        Parameters(params): Parameters<RunSceneParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);

        if std::env::var("POWERTOOL_SKIP_VALIDATION").ok().as_deref() != Some("1") {
            self.validate_scene_scripts(params.scene.as_deref(), timeout).await?;
        }

        let mut p = serde_json::json!({});
        if let Some(ref scene) = params.scene {
            p["scene"] = serde_json::json!(scene);
        }
        let result = self.run_via_editor("run_scene", p, timeout).await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Discover scripts referenced by a scene + project autoloads, validate them
    /// via Godot's LSP (with `--check-only` fallback), and return an error if any
    /// script fails to parse.
    async fn validate_scene_scripts(
        &self,
        scene: Option<&str>,
        timeout: Duration,
    ) -> Result<(), ErrorData> {
        // Resolve project root by asking the editor.
        let state = self
            .run_via_editor("get_editor_state", serde_json::json!({}), timeout)
            .await?;
        let project_root = match state.get("project_path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => return Ok(()), // Editor didn't report a project — skip silently.
        };
        let project_godot = project_root.join("project.godot");

        // Find the scene path: explicit param, or main_scene from project.godot.
        let scene_res = match scene {
            Some(s) => Some(s.to_string()),
            None => project::parse_main_scene(&project_godot)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?,
        };

        // Collect scripts: autoloads + scene's ext_resource Scripts.
        let mut script_paths: Vec<String> = project::parse_autoload_scripts(&project_godot)
            .unwrap_or_default();
        if let Some(ref s) = scene_res {
            let scene_fs = project::resolve_res_path(&project_root, s);
            if scene_fs.exists() {
                if let Ok(scripts) = project::parse_scene_scripts(&scene_fs) {
                    script_paths.extend(scripts);
                }
            }
        }
        script_paths.sort();
        script_paths.dedup();
        if script_paths.is_empty() {
            return Ok(());
        }

        // Build (uri, content) pairs.
        let mut files: Vec<lsp_validate::ScriptFile> = Vec::new();
        for res_path in &script_paths {
            let fs_path = project::resolve_res_path(&project_root, res_path);
            let Ok(text) = fs::read_to_string(&fs_path) else {
                continue;
            };
            let uri = format!("file://{}", fs_path.display());
            files.push(lsp_validate::ScriptFile { uri, text });
        }
        if files.is_empty() {
            return Ok(());
        }

        let lsp_port = std::env::var("POWERTOOL_LSP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_LSP_PORT);

        // Try LSP first. Fall back to --check-only if it can't be reached or errors.
        let lsp_result = lsp_validate::validate(
            "127.0.0.1",
            lsp_port,
            files,
            Duration::from_millis(800),
        )
        .await;

        let errors: Vec<serde_json::Value> = match lsp_result {
            Ok(diags) => lsp_validate::errors_only(&diags)
                .into_iter()
                .map(|d| {
                    serde_json::json!({
                        "uri": d.uri,
                        "line": d.line + 1,
                        "column": d.column + 1,
                        "message": d.message,
                        "source": "lsp",
                    })
                })
                .collect(),
            Err(e) => {
                tracing::warn!("LSP validation unavailable ({e}); falling back to --check-only");
                self.fallback_check_only(&project_root, &script_paths).await
            }
        };

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ErrorData::invalid_params(
                "GDScript parse errors prevent running the scene. Fix the listed errors and try again, or set POWERTOOL_SKIP_VALIDATION=1 to bypass.",
                Some(serde_json::json!({ "errors": errors })),
            ))
        }
    }

    async fn fallback_check_only(
        &self,
        project_root: &Path,
        script_paths: &[String],
    ) -> Vec<serde_json::Value> {
        let mut errors = Vec::new();
        for res_path in script_paths {
            let output = tokio::process::Command::new(&self.godot_path)
                .args([
                    "--headless",
                    "--path",
                    &project_root.to_string_lossy(),
                    "--check-only",
                    "--script",
                    res_path,
                ])
                .output()
                .await;
            let Ok(out) = output else { continue };
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                errors.push(serde_json::json!({
                    "uri": res_path,
                    "message": stderr.trim(),
                    "source": "check-only",
                }));
            }
        }
        errors
    }

    /// Stop the scene currently running in the Godot editor.
    #[rmcp::tool]
    async fn stop_scene(
        &self,
        Parameters(params): Parameters<EditorTimeoutParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);
        let result = self
            .run_via_editor("stop_scene", serde_json::json!({}), timeout)
            .await?;
        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }

    /// Take a screenshot of the running game viewport or editor viewport. Requires the editor to be running with the PowerTool addon. Use pause_first=true for CPU-heavy scenes where the debugger channel may be starved.
    #[rmcp::tool]
    async fn take_screenshot(
        &self,
        Parameters(params): Parameters<TakeScreenshotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.require_editor().await?;
        let timeout = Self::timeout_from(params.timeout_seconds);

        // Pause game tree before capture if requested
        if params.pause_first.unwrap_or(false) {
            let _ = self.run_via_editor("pause_for_screenshot", serde_json::json!({}), timeout).await;
        }

        let result = self
            .run_via_editor("take_screenshot", serde_json::json!({}), timeout)
            .await;

        // Resume game tree after capture
        if params.pause_first.unwrap_or(false) {
            let _ = self.run_via_editor("resume_after_screenshot", serde_json::json!({}), timeout).await;
        }

        let result = result?;

        // Editor returns base64 PNG in result.image_base64
        if let Some(b64) = result.get("image_base64").and_then(|v| v.as_str()) {
            if let Some(ref save_path) = params.file_path {
                use base64::Engine;
                let data = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| ErrorData::internal_error(format!("Base64 decode error: {e}"), None))?;
                fs::write(save_path, &data)
                    .map_err(|e| ErrorData::internal_error(format!("Failed to save: {e}"), None))?;
                return Ok(self.text_response(format!(
                    "Screenshot saved to: {save_path}"
                )));
            }
            let mut blocks = Vec::new();
            if let Some(w) = self.multi_instance_warning() {
                blocks.push(Content::text(w));
            }
            blocks.push(Content::image(b64.to_string(), "image/png"));
            return Ok(CallToolResult::success(blocks));
        }

        Ok(self.text_response(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }
}

#[rmcp::tool_handler]
impl ServerHandler for GodotMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.instructions = Some(
            "Godot game engine MCP server. Provides tools for managing Godot projects, \
             scenes, nodes, and running/debugging games."
                .to_string(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}
