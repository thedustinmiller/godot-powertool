use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};

/// Fetch Godot class documentation XML files via sparse git clone.
/// Returns the path to the XML class files directory.
pub fn fetch_godot_docs(version: &str, output_dir: &Path) -> Result<PathBuf> {
    let xml_dir = output_dir.join("godot").join("doc").join("classes");

    // Idempotent: skip if already fetched
    if xml_dir.exists() && std::fs::read_dir(&xml_dir)?.count() > 0 {
        println!("Godot docs already fetched at {}", xml_dir.display());
        return Ok(xml_dir);
    }

    std::fs::create_dir_all(output_dir)?;

    let branch = format!("{version}-stable");
    println!("Fetching Godot {version} class documentation...");

    let clone_dir = output_dir.join("godot");
    if clone_dir.exists() {
        std::fs::remove_dir_all(&clone_dir)?;
    }

    // Sparse clone: only fetch doc/classes/ from the specific version tag
    let status = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--filter=blob:none",
            "--sparse",
            "--branch",
            &branch,
            "https://github.com/godotengine/godot.git",
        ])
        .arg(&clone_dir)
        .status()
        .context("Failed to run git clone")?;

    if !status.success() {
        bail!("git clone failed for branch {branch}");
    }

    let status = Command::new("git")
        .args(["sparse-checkout", "set", "doc/classes"])
        .current_dir(&clone_dir)
        .status()
        .context("Failed to set sparse checkout")?;

    if !status.success() {
        bail!("git sparse-checkout failed");
    }

    if !xml_dir.exists() {
        bail!(
            "Expected XML docs at {} but directory not found after clone",
            xml_dir.display()
        );
    }

    let count = std::fs::read_dir(&xml_dir)?.count();
    println!("Fetched {count} class XML files");

    Ok(xml_dir)
}
