use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

/// Information about a Godot project discovered on disk.
#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub name: String,
    pub path: PathBuf,
    pub godot_version: Option<String>,
    pub scenes: Vec<PathBuf>,
    pub scripts: Vec<PathBuf>,
}

/// Parse a project.godot file to extract the project name.
pub fn parse_project_name(project_godot: &Path) -> Result<String> {
    let content = fs::read_to_string(project_godot)
        .with_context(|| format!("Failed to read {}", project_godot.display()))?;

    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("config/name=") {
            let name = rest.trim().trim_matches('"');
            return Ok(name.to_string());
        }
    }

    Ok("Unknown Project".to_string())
}

/// Get detailed info about a Godot project at the given directory.
pub fn get_project_info(project_dir: &Path) -> Result<ProjectInfo> {
    let project_file = project_dir.join("project.godot");
    if !project_file.exists() {
        anyhow::bail!("Not a Godot project: {}", project_dir.display());
    }

    let name = parse_project_name(&project_file)?;

    let content = fs::read_to_string(&project_file)?;
    let godot_version = content
        .lines()
        .find(|l| l.starts_with("config/features="))
        .and_then(|l| {
            // Extract version from: config/features=PackedStringArray("4.3", "Forward Plus")
            l.split('"').nth(1).map(String::from)
        });

    let mut scenes = Vec::new();
    let mut scripts = Vec::new();

    if let Ok(entries) = walkdir::WalkDir::new(project_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden dirs, addons, .godot cache
            !name.starts_with('.') && name != "addons"
        })
        .collect::<Result<Vec<_>, _>>()
    {
        for entry in entries {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                match ext {
                    "tscn" | "scn" => scenes.push(path.to_path_buf()),
                    "gd" => scripts.push(path.to_path_buf()),
                    _ => {}
                }
            }
        }
    }

    Ok(ProjectInfo {
        name,
        path: project_dir.to_path_buf(),
        godot_version,
        scenes,
        scripts,
    })
}

/// Find Godot projects in a directory, optionally recursing.
pub fn find_projects(dir: &Path, recursive: bool) -> Vec<ProjectInfo> {
    let mut projects = Vec::new();

    if dir.join("project.godot").exists() {
        if let Ok(info) = get_project_info(dir) {
            projects.push(info);
        }
    }

    if recursive {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    // Skip hidden dirs, node_modules, target, etc.
                    if !name.starts_with('.') && name != "node_modules" && name != "target" {
                        projects.extend(find_projects(&path, true));
                    }
                }
            }
        }
    }

    projects
}
