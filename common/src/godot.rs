use std::{
    path::Path,
    process::{Child, Command, Output, Stdio},
};

use anyhow::{Context, Result, bail};

/// Run Godot synchronously with given arguments, returning the full output.
pub fn run_godot(binary: &Path, args: &[&str], cwd: &Path) -> Result<Output> {
    Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run Godot with args: {args:?}"))
}

/// Run Godot in headless mode with given arguments.
pub fn run_godot_headless(binary: &Path, args: &[&str], cwd: &Path) -> Result<Output> {
    let mut full_args = vec!["--headless"];
    full_args.extend_from_slice(args);
    run_godot(binary, &full_args, cwd)
}

/// Run a GDScript operation via `godot_operations.gd`.
///
/// Invokes: `godot --headless --path <project> --script <script> <operation> <json_params>`
///
/// The JSON params are shell-escaped appropriately for the current OS.
pub fn run_godot_operation(
    binary: &Path,
    project: &Path,
    script: &Path,
    op: &str,
    params: &serde_json::Value,
) -> Result<String> {
    let params_str = serde_json::to_string(params)?;

    let output = Command::new(binary)
        .args([
            "--headless",
            "--path",
            &project.to_string_lossy(),
            "--script",
            &script.to_string_lossy(),
            op,
            &params_str,
        ])
        .output()
        .with_context(|| format!("Failed to run Godot operation: {op}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        bail!(
            "Godot operation '{op}' failed (exit code: {:?}):\nstdout: {stdout}\nstderr: {stderr}",
            output.status.code()
        );
    }

    Ok(stdout)
}

/// Spawn Godot as a non-blocking child process (for `run_project`, `launch_editor`, etc.).
pub fn spawn_godot(binary: &Path, args: &[&str], cwd: &Path) -> Result<Child> {
    Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to spawn Godot with args: {args:?}"))
}

/// Run Godot --version and return the version string.
pub fn get_godot_version(binary: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .context("Failed to get Godot version")?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
