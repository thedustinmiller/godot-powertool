use std::{
    path::Path,
    process::{Child, Command, Output, Stdio},
    time::Duration,
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

/// Spawn Godot as a non-blocking child process (for `run_scene`, `launch_editor`, etc.).
///
/// `stdout` and `stderr` control where the child's output streams go:
/// - `Stdio::null()` — discard output (best for long-lived processes like the editor)
/// - `Stdio::piped()` — capture output for the parent to read
/// - `Stdio::inherit()` — forward to the parent's own streams
pub fn spawn_godot(
    binary: &Path,
    args: &[&str],
    cwd: &Path,
    stdout: Stdio,
    stderr: Stdio,
) -> Result<Child> {
    Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .stdout(stdout)
        .stderr(stderr)
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

/// Default timeout for Godot operations.
pub const DEFAULT_TIMEOUT_SECS: u64 = 15;

/// Run a Godot command asynchronously with a timeout.
///
/// Spawns the process via `tokio::process::Command`, then races
/// `child.wait_with_output()` against `tokio::time::timeout`.
/// On timeout the child is killed before returning an error.
async fn run_with_timeout(
    child: tokio::process::Child,
    timeout: Duration,
    label: &str,
) -> Result<Output> {
    // child was spawned with kill_on_drop(true) so if the timeout fires
    // and the future (which owns the child) is dropped, the process is killed.
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(result) => result.with_context(|| format!("Failed to wait on Godot process: {label}")),
        Err(_) => {
            bail!("{label} timed out after {:.0}s", timeout.as_secs_f64());
        }
    }
}

/// Async version of [`run_godot_operation`] with a configurable timeout.
pub async fn run_godot_operation_async(
    binary: &Path,
    project: &Path,
    script: &Path,
    op: &str,
    params: &serde_json::Value,
    timeout: Duration,
) -> Result<String> {
    let params_str = serde_json::to_string(params)?;

    let child = tokio::process::Command::new(binary)
        .args([
            "--headless",
            "--path",
            &project.to_string_lossy(),
            "--script",
            &script.to_string_lossy(),
            op,
            &params_str,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to spawn Godot operation: {op}"))?;

    let output = run_with_timeout(child, timeout, &format!("Godot operation '{op}'")).await?;

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

/// Async version of [`get_godot_version`] with a configurable timeout.
pub async fn get_godot_version_async(binary: &Path, timeout: Duration) -> Result<String> {
    let child = tokio::process::Command::new(binary)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("Failed to spawn Godot for --version")?;

    let output = run_with_timeout(child, timeout, "godot --version").await?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
