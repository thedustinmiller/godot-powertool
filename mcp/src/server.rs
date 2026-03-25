use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::Arc,
    time::Duration,
};

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
    platform::{find_godot_binary, godot_user_data_dir},
    project,
};

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

/// Active Godot process state
struct GodotProcess {
    child: Child,
    output: Vec<String>,
    errors: Vec<String>,
}

pub struct GodotMcpServer {
    godot_path: PathBuf,
    active_process: Arc<Mutex<Option<GodotProcess>>>,
    operations_script: PathBuf,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

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

        Ok(Self {
            godot_path,
            active_process: Arc::new(Mutex::new(None)),
            operations_script,
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
struct RunProjectParams {
    /// Path to the Godot project directory
    project_path: String,
    /// Specific scene to run (optional)
    scene: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListProjectsParams {
    /// Directory to search for Godot projects
    directory: String,
    /// Search recursively in subdirectories
    recursive: Option<bool>,
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
    /// Path to the Godot project directory (optional, uses running project)
    project_path: Option<String>,
    /// File path to save screenshot to (optional, returns base64 if not set)
    file_path: Option<String>,
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
        Ok(CallToolResult::success(vec![Content::text(version)]))
    }

    /// Launch the Godot editor for a project
    #[rmcp::tool]
    async fn launch_editor(
        &self,
        Parameters(params): Parameters<ProjectPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        godot_cli::spawn_godot(&self.godot_path, &["-e", "--path", &project.to_string_lossy()], &project)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Launched Godot editor for {}",
            params.project_path
        ))]))
    }

    /// Run a Godot project in debug mode
    #[rmcp::tool]
    async fn run_project(
        &self,
        Parameters(params): Parameters<RunProjectParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;

        // Kill existing process
        {
            let mut active = self.active_process.lock().await;
            if let Some(mut proc) = active.take() {
                let _ = proc.child.kill();
            }
        }

        let project_str = project.to_string_lossy().to_string();
        let mut args = vec!["-d", "--path", project_str.as_str()];
        let scene_str;
        if let Some(ref scene) = params.scene {
            scene_str = scene.clone();
            args.push(&scene_str);
        }

        let child = std::process::Command::new(&self.godot_path)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        {
            let mut active = self.active_process.lock().await;
            *active = Some(GodotProcess {
                child,
                output: Vec::new(),
                errors: Vec::new(),
            });
        }

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Started Godot project: {}",
            params.project_path
        ))]))
    }

    /// Get debug output from the running Godot project
    #[rmcp::tool]
    async fn get_debug_output(&self) -> Result<CallToolResult, ErrorData> {
        let active = self.active_process.lock().await;
        match &*active {
            Some(proc) => {
                let output = serde_json::json!({
                    "output": proc.output,
                    "errors": proc.errors,
                });
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&output).unwrap_or_default(),
                )]))
            }
            None => Err(ErrorData::invalid_params(
                "No Godot project is currently running",
                None,
            )),
        }
    }

    /// Stop the currently running Godot project
    #[rmcp::tool]
    async fn stop_project(&self) -> Result<CallToolResult, ErrorData> {
        let mut active = self.active_process.lock().await;
        match active.take() {
            Some(mut proc) => {
                let _ = proc.child.kill();
                let final_output = serde_json::json!({
                    "message": "Project stopped",
                    "final_output": proc.output,
                    "final_errors": proc.errors,
                });
                Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&final_output).unwrap_or_default(),
                )]))
            }
            None => Ok(CallToolResult::success(vec![Content::text(
                "No project was running",
            )])),
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

        let projects = project::find_projects(&dir, params.recursive.unwrap_or(false));
        let result: Vec<serde_json::Value> = projects
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "path": p.path.to_string_lossy(),
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        )]))
    }

    /// Get detailed information about a Godot project
    #[rmcp::tool]
    async fn get_project_info(
        &self,
        Parameters(params): Parameters<ProjectPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let project = Self::validate_project_path(&params.project_path)?;
        let info = project::get_project_info(&project)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let result = serde_json::json!({
            "name": info.name,
            "path": info.path.to_string_lossy(),
            "godot_version": info.godot_version,
            "scenes": info.scenes.iter().map(|s| s.to_string_lossy().to_string()).collect::<Vec<_>>(),
            "scripts": info.scripts.iter().map(|s| s.to_string_lossy().to_string()).collect::<Vec<_>>(),
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        )]))
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
        Ok(CallToolResult::success(vec![Content::text(output)]))
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
        Ok(CallToolResult::success(vec![Content::text(output)]))
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
        Ok(CallToolResult::success(vec![Content::text(output)]))
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
        Ok(CallToolResult::success(vec![Content::text(output)]))
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
        Ok(CallToolResult::success(vec![Content::text(output)]))
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
        Ok(CallToolResult::success(vec![Content::text(output)]))
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
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Take a screenshot of the running Godot project
    #[rmcp::tool]
    async fn take_screenshot(
        &self,
        Parameters(params): Parameters<TakeScreenshotParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Must have a running project
        {
            let active = self.active_process.lock().await;
            if active.is_none() {
                return Err(ErrorData::invalid_params(
                    "No Godot project is currently running. Start one with run_project first.",
                    None,
                ));
            }
        }

        // Determine project name for user data dir
        let project_path = params.project_path.as_deref().unwrap_or(".");
        let project_dir = PathBuf::from(project_path);
        let project_name = if project_dir.join("project.godot").exists() {
            project::parse_project_name(&project_dir.join("project.godot"))
                .unwrap_or_else(|_| "Unknown Project".to_string())
        } else {
            "Unknown Project".to_string()
        };

        let user_dir = godot_user_data_dir(&project_name);
        fs::create_dir_all(&user_dir)
            .map_err(|e| ErrorData::internal_error(format!("Failed to create user dir: {e}"), None))?;

        let request_file = user_dir.join("mcp_screenshot_request.txt");
        let output_file = user_dir.join("mcp_screenshot.png");

        // Remove existing screenshot
        let _ = fs::remove_file(&output_file);

        // Write request
        fs::write(&request_file, "take_screenshot")
            .map_err(|e| ErrorData::internal_error(format!("Failed to write request: {e}"), None))?;

        // Poll for result (5 second timeout, 100ms intervals)
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if output_file.exists() {
                break;
            }
        }

        if !output_file.exists() {
            return Err(ErrorData::internal_error(
                "Screenshot request timed out. Make sure the ScreenshotManager autoload is installed.",
                None,
            ));
        }

        let img_data = fs::read(&output_file)
            .map_err(|e| ErrorData::internal_error(format!("Failed to read screenshot: {e}"), None))?;

        // If file path specified, save there
        if let Some(ref save_path) = params.file_path {
            fs::write(save_path, &img_data)
                .map_err(|e| ErrorData::internal_error(format!("Failed to save screenshot: {e}"), None))?;
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Screenshot saved to: {save_path}"
            ))]));
        }

        // Return as base64 image
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&img_data);
        Ok(CallToolResult::success(vec![Content::image(b64, "image/png")]))
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
