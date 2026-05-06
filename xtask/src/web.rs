use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};

use super::{
    ExtensionPaths, cmd_build as cmd_xtask_build, extension_paths, godot_binary, godot_dir,
    project_root,
};

fn web_dir() -> Result<PathBuf> {
    Ok(project_root()?.join("web"))
}

fn npm_cmd() -> Result<PathBuf> {
    which::which("npm").context("npm not found. Please install Node.js from https://nodejs.org/")
}

pub(crate) fn cmd_setup() -> Result<()> {
    println!("Web frontend setup instructions:\n");

    let web = web_dir()?;

    print!("Node.js: ");
    match which::which("node") {
        Ok(_) => {
            let output = Command::new("node").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        }
        Err(_) => println!("NOT FOUND - install from https://nodejs.org/"),
    }

    print!("npm: ");
    match which::which("npm") {
        Ok(_) => {
            let output = Command::new("npm").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        }
        Err(_) => println!("NOT FOUND"),
    }

    print!("Dependencies: ");
    if web.join("node_modules").exists() {
        println!("installed");
    } else {
        println!("NOT INSTALLED");
    }

    println!("\nTo set up the web frontend, run these commands manually:");
    println!("  cd web && npm install");
    println!("  npx playwright install --with-deps chromium firefox");
    println!("\nThen you can:");
    println!("  cargo xtask web export   # Export Godot project for web");
    println!("  cargo xtask web dev      # Run development server");
    println!("  cargo xtask web test     # Run Playwright tests");

    Ok(())
}

pub(crate) fn cmd_dev(port: u16) -> Result<()> {
    let web = web_dir()?;
    let npm = npm_cmd()?;

    if !web.join("node_modules").exists() {
        bail!(
            "Dependencies not installed. Run:\n  cd web && npm install\n\
             Or see: cargo xtask web setup"
        );
    }

    println!("Starting Vite development server on port {port}...\n");
    println!("Note: Game files must be exported first with 'cargo xtask web export'\n");

    let status = Command::new(&npm)
        .args(["run", "dev", "--", "--port", &port.to_string()])
        .current_dir(&web)
        .status()
        .context("Failed to start Vite dev server")?;

    if !status.success() {
        bail!("Dev server exited with error");
    }

    Ok(())
}

pub(crate) fn cmd_build() -> Result<()> {
    let web = web_dir()?;
    let npm = npm_cmd()?;

    if !web.join("node_modules").exists() {
        bail!(
            "Dependencies not installed. Run:\n  cd web && npm install\n\
             Or see: cargo xtask web setup"
        );
    }

    println!("Building web frontend for production...\n");

    let status = Command::new(&npm)
        .args(["run", "build"])
        .current_dir(&web)
        .status()
        .context("Failed to build web frontend")?;

    if !status.success() {
        bail!("Web build failed");
    }

    println!("\nBuild complete! Output in web/dist/");
    Ok(())
}

const GODOT_EXPORT_EXTENSIONS: &[&str] = &["html", "js", "wasm", "pck", "png", "json", "worker.js"];

fn is_godot_export_file(path: &Path, ext: &ExtensionPaths) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    name.starts_with("godot.")
        || name == ext.wasm_artifact(false)
        || name == ext.wasm_artifact(true)
        || GODOT_EXPORT_EXTENSIONS
            .iter()
            .any(|e| name.ends_with(e) && name.starts_with("godot"))
}

pub(crate) fn cmd_export(release: bool) -> Result<()> {
    println!("Exporting Godot project for web...\n");

    let ext = extension_paths()?;
    let godot_path = godot_dir()?;
    let web = web_dir()?;
    let export_dir = web.join("public");

    println!("Cleaning previous Godot export artifacts...");
    if export_dir.exists() {
        for entry in fs::read_dir(&export_dir)? {
            let path = entry?.path();
            if path.is_file() && is_godot_export_file(&path, &ext) {
                fs::remove_file(&path)?;
            }
        }
    }
    let old_game_dir = export_dir.join("game");
    if old_game_dir.exists() {
        println!("Removing old web/public/game/ directory...");
        fs::remove_dir_all(&old_game_dir)?;
    }

    fs::create_dir_all(&export_dir)?;

    println!("Step 1: Building native GDExtension (for export process)...");
    cmd_xtask_build(release, false, false, false)?;

    println!("\nStep 2: Building WASM GDExtension...");
    match cmd_xtask_build(release, true, false, false) {
        Ok(()) => println!("WASM extension built successfully!"),
        Err(e) => {
            println!("Warning: WASM build failed: {e}");
            println!("Continuing with export (GDScript will work, Rust extension will not)...\n");
        }
    }

    let godot = godot_binary()?;

    println!("Checking for web export templates...");
    let templates_dir = dirs_next::data_dir()
        .map(|d| d.join("godot").join("export_templates"))
        .unwrap_or_else(|| PathBuf::from("~/.local/share/godot/export_templates"));

    if !templates_dir.exists() {
        println!(
            "Warning: Export templates directory not found at {}",
            templates_dir.display()
        );
        println!("You may need to download export templates from the Godot editor.");
        println!("  Editor -> Export -> Download export templates");
    }

    println!("\nExporting Godot project...");
    let export_path = export_dir.join("godot.html");

    let status = Command::new(&godot)
        .args([
            "--headless",
            "--export-release",
            "Web",
            &export_path.to_string_lossy(),
        ])
        .current_dir(&godot_path)
        .status()
        .context("Failed to export Godot project")?;

    if !status.success() {
        println!("Release export failed, trying debug export...");
        let status = Command::new(&godot)
            .args([
                "--headless",
                "--export-debug",
                "Web",
                &export_path.to_string_lossy(),
            ])
            .current_dir(&godot_path)
            .status()
            .context("Failed to export Godot project")?;

        if !status.success() {
            bail!(
                "Godot web export failed.\n\
                Make sure you have:\n\
                  1. Web export templates installed\n\
                  2. Export preset configured in godot/export_presets.cfg\n\
                You can install templates from: Editor -> Export -> Download Templates"
            );
        }
    }

    if export_dir.join("godot.js").exists() {
        println!("\nExport successful!");
        println!("Files exported to: {}", export_dir.display());
    } else {
        println!("\nWarning: Export may have partially failed.");
        println!("Check {} for output files.", export_dir.display());
    }

    println!("\nYou can now run: cargo xtask web dev");

    Ok(())
}

pub(crate) fn cmd_test(headed: bool, ui: bool, debug: bool, filter: Option<&str>) -> Result<()> {
    let web = web_dir()?;

    if !web.join("node_modules").exists() {
        bail!(
            "Dependencies not installed. Run:\n  cd web && npm install\n\
             npx playwright install --with-deps chromium firefox\n\
             Or see: cargo xtask web setup"
        );
    }

    println!("Running Playwright tests...\n");

    let mut cmd = Command::new("npx");
    cmd.arg("playwright").arg("test");

    if ui {
        cmd.arg("--ui");
    } else if debug {
        cmd.arg("--debug");
    } else if headed {
        cmd.arg("--headed");
    }

    if let Some(f) = filter {
        cmd.arg("--grep").arg(f);
    }

    cmd.current_dir(&web);

    let status = cmd.status().context("Failed to run Playwright tests")?;

    if !status.success() {
        bail!("Playwright tests failed");
    }

    println!("\nPlaywright tests passed!");
    Ok(())
}

pub(crate) fn cmd_preview(port: u16) -> Result<()> {
    let web = web_dir()?;
    let npm = npm_cmd()?;

    if !web.join("dist").exists() {
        println!("Production build not found. Building first...\n");
        cmd_build()?;
        println!();
    }

    println!("Starting preview server on port {port}...\n");

    let status = Command::new(&npm)
        .args(["run", "preview", "--", "--port", &port.to_string()])
        .current_dir(&web)
        .status()
        .context("Failed to start preview server")?;

    if !status.success() {
        bail!("Preview server exited with error");
    }

    Ok(())
}

pub(crate) fn cmd_doctor() -> Result<()> {
    println!("Checking web project health...\n");

    let web = web_dir()?;
    let mut issues = Vec::new();

    print!("Node.js: ");
    match which::which("node") {
        Ok(_) => {
            let output = Command::new("node").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        }
        Err(_) => {
            println!("NOT FOUND");
            issues.push("Node.js not installed. Visit https://nodejs.org/");
        }
    }

    print!("npm: ");
    match which::which("npm") {
        Ok(_) => {
            let output = Command::new("npm").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        }
        Err(_) => {
            println!("NOT FOUND");
            issues.push("npm not installed");
        }
    }

    print!("Dependencies: ");
    if web.join("node_modules").exists() {
        println!("installed");
    } else {
        println!("NOT INSTALLED");
        issues.push("Run 'cargo xtask web setup' to install dependencies");
    }

    print!("Playwright: ");
    let playwright_check = Command::new("npx")
        .args(["playwright", "--version"])
        .current_dir(&web)
        .output();

    match playwright_check {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            println!("{}", version.trim());
        }
        _ => {
            println!("NOT INSTALLED");
            issues.push("Run 'cargo xtask web setup' to install Playwright");
        }
    }

    print!("Game export: ");
    let godot_js = web.join("public").join("godot.js");
    if godot_js.exists() {
        println!("found in web/public/");
    } else {
        println!("NOT FOUND");
        issues.push("Run 'cargo xtask web export' to export game for web");
    }

    print!("Production build: ");
    if web.join("dist").exists() {
        println!("found at web/dist/");
    } else {
        println!("NOT BUILT");
        issues.push("Run 'cargo xtask web build' to create production build");
    }

    println!();

    if issues.is_empty() {
        println!("All web checks passed!");
    } else {
        println!("Issues found:");
        for issue in &issues {
            println!("  - {issue}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extension_paths_for_test() -> ExtensionPaths {
        ExtensionPaths {
            crate_name: "sample_extension".to_string(),
            crate_path: "addons/sample_extension/rust".to_string(),
            deploy_path: PathBuf::from("godot/addons/sample_extension/bin"),
            lib_basename: "sample_extension".to_string(),
        }
    }

    #[test]
    fn identifies_godot_export_artifacts() {
        let ext = extension_paths_for_test();
        for name in [
            "godot.html",
            "godot.js",
            "godot.wasm",
            "godot.pck",
            "godot.worker.js",
            "sample_extension.threads.wasm",
            "sample_extension.wasm",
        ] {
            assert!(is_godot_export_file(Path::new(name), &ext), "{name}");
        }
    }

    #[test]
    fn leaves_non_export_files_alone() {
        let ext = extension_paths_for_test();
        for name in [
            "index.html",
            "app.js",
            "godot_notes.txt",
            "sample_extension.dll",
        ] {
            assert!(!is_godot_export_file(Path::new(name), &ext), "{name}");
        }
    }
}
