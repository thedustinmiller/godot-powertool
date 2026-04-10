//! Project management CLI for Godot projects with optional Rust GDExtension.
//!
//! Usage: cargo xtask <command>

use std::{
    env, fs,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use indicatif::{ProgressBar, ProgressStyle};
use powertool_common::{
    config::load_template_config,
    platform::{
        extension_lib_name, extension_platform_dir, find_godot_binary, godot_download_info,
        open_path,
    },
    skill::SkillTarget,
};

/// Godot project manager
#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Project management CLI for Godot projects")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// First-time project setup — downloads Godot, assets, and configures all tooling
    Init {
        /// Godot version to download (overrides template.toml)
        #[arg(long)]
        godot_version: Option<String>,

        /// Skill install target: claude, codex, generic, or a custom path
        #[arg(long, default_value = "claude")]
        skill_target: String,
    },

    /// Re-setup tooling — Godot (version-aware), extension, import, docs, skill, MCP
    Setup {
        /// Godot version to download (overrides template.toml)
        #[arg(long)]
        godot_version: Option<String>,

        /// Skip Godot download (use system Godot)
        #[arg(long)]
        skip_godot: bool,

        /// Skip generating Godot API docs
        #[arg(long)]
        skip_docs: bool,

        /// Skip installing agent skill files
        #[arg(long)]
        skip_skill: bool,

        /// Skip building the MCP server
        #[arg(long)]
        skip_mcp: bool,

        /// Skill install target: claude, codex, generic, or a custom path
        #[arg(long, default_value = "claude")]
        skill_target: String,
    },

    /// Fetch and extract assets from template.toml into the project
    Update,

    /// Build the GDExtension library
    Build {
        /// Build in release mode (default: debug)
        #[arg(long, short)]
        release: bool,

        /// Build for web/WASM target (nothreads by default)
        #[arg(long)]
        web: bool,

        /// Also build the threaded WASM variant (implies --web)
        #[arg(long)]
        threads: bool,
    },

    /// Run the Godot project
    Run {
        /// Open the Godot editor instead of running the game
        #[arg(long, short)]
        editor: bool,

        /// Additional arguments to pass to Godot
        #[arg(last = true)]
        godot_args: Vec<String>,
    },

    /// Run tests
    Test {
        /// Run only Rust tests
        #[arg(long)]
        rust_only: bool,

        /// Run only Godot GUT tests
        #[arg(long)]
        godot_only: bool,

        /// Verbose test output
        #[arg(long, short)]
        verbose: bool,

        /// Filter tests by pattern
        #[arg(long, short)]
        filter: Option<String>,
    },

    /// Clean build artifacts
    Clean {
        /// Also remove downloaded tools (Godot)
        #[arg(long)]
        all: bool,
    },

    /// Check project health and dependencies
    Doctor,

    /// Format all code (Rust and GDScript)
    Fmt {
        /// Check formatting without making changes
        #[arg(long)]
        check: bool,
    },

    /// Run linters on all code
    Lint,

    /// Run full CI pipeline (fmt check, lint, tests)
    Ci,

    /// Generate and open documentation
    Docs {
        /// Don't open docs in browser
        #[arg(long)]
        no_open: bool,
    },

    /// Open the Godot editor
    Editor,

    /// Build and run (development iteration)
    Dev {
        /// Build in release mode
        #[arg(long, short)]
        release: bool,
    },

    /// Web frontend commands (Vite + Playwright)
    #[command(subcommand)]
    Web(WebCommands),

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Install/manage agent skill files
    #[command(subcommand)]
    Skill(SkillCommands),

    /// Launch or configure the MCP server
    #[command(subcommand)]
    Mcp(McpCommands),

    /// Launch or configure the GDScript LSP bridge
    #[command(subcommand)]
    LspBridge(LspBridgeCommands),
}

#[derive(Subcommand)]
enum WebCommands {
    /// Check web prerequisites and print setup instructions
    Setup,

    /// Run Vite development server
    Dev {
        /// Port to run on
        #[arg(long, short, default_value = "3000")]
        port: u16,
    },

    /// Build the web frontend for production
    Build,

    /// Export Godot project for web (includes WASM extension)
    Export {
        /// Build in release mode
        #[arg(long, short)]
        release: bool,
    },

    /// Run Playwright tests
    Test {
        /// Run tests in headed mode (show browser)
        #[arg(long)]
        headed: bool,

        /// Run tests in UI mode
        #[arg(long)]
        ui: bool,

        /// Run tests in debug mode
        #[arg(long)]
        debug: bool,

        /// Filter tests by pattern
        #[arg(long, short)]
        filter: Option<String>,
    },

    /// Preview production build
    Preview {
        /// Port to run on
        #[arg(long, short, default_value = "3000")]
        port: u16,
    },

    /// Check web project health
    Doctor,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// Install skill files into the project
    Install {
        /// Target agent platform: claude, codex, generic, or a custom path
        #[arg(long, short, default_value = "claude")]
        target: String,
    },

    /// Update skill files (regenerate docs + reinstall)
    Update {
        /// Target agent platform
        #[arg(long, short, default_value = "claude")]
        target: String,
    },

    /// Remove installed skill files
    Remove {
        /// Target agent platform
        #[arg(long, short, default_value = "claude")]
        target: String,
    },
}

#[derive(Subcommand)]
enum McpCommands {
    /// Run the MCP server on stdio
    Run,

    /// Print MCP configuration for various clients
    Install {
        /// Client to generate config for: claude, cursor, cline
        #[arg(default_value = "claude")]
        client: String,
    },
}

#[derive(Subcommand)]
enum LspBridgeCommands {
    /// Run the GDScript LSP bridge on stdio
    Run,

    /// Print LSP bridge configuration for various clients
    Install {
        /// Client to generate config for: claude
        #[arg(default_value = "claude")]
        client: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            godot_version,
            skill_target,
        } => {
            let config = load_template_config(&project_root()?)?;
            let godot_v = godot_version.unwrap_or(config.godot.clone());
            cmd_init(&godot_v, &skill_target, &config.assets)
        },
        Commands::Setup {
            godot_version,
            skip_godot,
            skip_docs,
            skip_skill,
            skip_mcp,
            skill_target,
        } => {
            let config = load_template_config(&project_root()?)?;
            let godot_v = godot_version.unwrap_or(config.godot.clone());
            cmd_setup(&SetupOptions {
                godot_version: godot_v,
                skip_godot,
                skip_docs,
                skip_skill,
                skip_mcp,
                skill_target,
            })
        },
        Commands::Update => {
            let config = load_template_config(&project_root()?)?;
            cmd_update(&config.assets)
        },
        Commands::Build {
            release,
            web,
            threads,
        } => cmd_build(release, web || threads, threads),
        Commands::Run { editor, godot_args } => cmd_run(editor, &godot_args),
        Commands::Test {
            rust_only,
            godot_only,
            verbose,
            filter,
        } => cmd_test(rust_only, godot_only, verbose, filter.as_deref()),
        Commands::Clean { all } => cmd_clean(all),
        Commands::Doctor => cmd_doctor(),
        Commands::Fmt { check } => cmd_fmt(check),
        Commands::Lint => cmd_lint(),
        Commands::Ci => cmd_ci(),
        Commands::Docs { no_open } => cmd_docs(no_open),
        Commands::Editor => cmd_editor(),
        Commands::Dev { release } => cmd_dev(release),
        Commands::Completions { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "xtask", &mut io::stdout());
            Ok(())
        },
        Commands::Web(web_cmd) => match web_cmd {
            WebCommands::Setup => cmd_web_setup(),
            WebCommands::Dev { port } => cmd_web_dev(port),
            WebCommands::Build => cmd_web_build(),
            WebCommands::Export { release } => cmd_web_export(release),
            WebCommands::Test {
                headed,
                ui,
                debug,
                filter,
            } => cmd_web_test(headed, ui, debug, filter.as_deref()),
            WebCommands::Preview { port } => cmd_web_preview(port),
            WebCommands::Doctor => cmd_web_doctor(),
        },
        Commands::Skill(cmd) => match cmd {
            SkillCommands::Install { target } => cmd_skill_install(&target),
            SkillCommands::Update { target } => cmd_skill_update(&target),
            SkillCommands::Remove { target } => cmd_skill_remove(&target),
        },
        Commands::Mcp(cmd) => match cmd {
            McpCommands::Run => cmd_mcp_run(),
            McpCommands::Install { client } => cmd_mcp_install(&client),
        },
        Commands::LspBridge(cmd) => match cmd {
            LspBridgeCommands::Run => cmd_lsp_bridge_run(),
            LspBridgeCommands::Install { client } => cmd_lsp_bridge_install(&client),
        },
    }
}

// =============================================================================
// Path utilities (project-specific, delegates to common for cross-platform)
// =============================================================================

fn project_root() -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest_dir)
        .parent()
        .context("Failed to find project root")?;
    Ok(root.to_path_buf())
}

fn godot_dir() -> Result<PathBuf> {
    Ok(project_root()?.join("godot"))
}

fn tools_dir() -> Result<PathBuf> {
    Ok(project_root()?.join("tools"))
}

fn godot_binary() -> Result<PathBuf> {
    let root = project_root()?;
    find_godot_binary(Some(&root))
}

fn extension_bin_dir() -> Result<PathBuf> {
    Ok(godot_dir()?
        .join("addons")
        .join("extension")
        .join("bin")
        .join(extension_platform_dir()))
}

/// Copy the contents of `<root>/addons/` into `<root>/godot/addons/`.
fn copy_addons(root: &Path) -> Result<()> {
    let src = root.join("addons");
    if !src.exists() {
        return Ok(());
    }
    let dst = root.join("godot").join("addons");
    fs::create_dir_all(&dst)?;
    println!("Copying addons into godot/addons/...");
    copy_dir_recursive(&src, &dst)?;
    Ok(())
}

/// Check if "extension" is listed in workspace members in root Cargo.toml.
fn extension_enabled() -> bool {
    let Ok(root) = project_root() else {
        return false;
    };
    let Ok(content) = fs::read_to_string(root.join("Cargo.toml")) else {
        return false;
    };
    let Ok(toml) = content.parse::<toml::Value>() else {
        return false;
    };
    toml.get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .is_some_and(|members| {
            members
                .iter()
                .any(|v| v.as_str().is_some_and(|s| s == "extension"))
        })
}

// =============================================================================
// Commands
// =============================================================================

struct SetupOptions {
    godot_version: String,
    skip_godot: bool,
    skip_docs: bool,
    skip_skill: bool,
    skip_mcp: bool,
    skill_target: String,
}

// =============================================================================
// User prompts
// =============================================================================

/// Prompt the user for y/n confirmation. Returns true on 'y'.
fn confirm_prompt(message: &str) -> Result<bool> {
    eprint!("{message} [y/n]: ");
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

/// Prompt the user for y/n/s. Returns the lowercase char. Loops on invalid input.
fn confirm_prompt_yns(message: &str) -> Result<char> {
    loop {
        eprint!("{message} [y/n/s]: ");
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        match input.trim().to_ascii_lowercase().chars().next() {
            Some(c @ ('y' | 'n' | 's')) => return Ok(c),
            _ => eprintln!("Please enter y, n, or s."),
        }
    }
}

// =============================================================================
// Asset zip helpers
// =============================================================================

/// Strip the top-level directory component from a zip entry name.
/// e.g. "Gut-9.6.0/addons/gut/plugin.cfg" -> "addons/gut/plugin.cfg"
/// Returns `None` for the root directory entry itself.
fn strip_root_component(name: &str) -> Option<&str> {
    let rest = if let Some(idx) = name.find('/') {
        &name[idx + 1..]
    } else {
        return None; // bare file at root level with no slash — keep as-is shouldn't happen
    };
    if rest.is_empty() {
        None // this was the root dir entry itself (e.g. "Gut-9.6.0/")
    } else {
        Some(rest)
    }
}

/// Collect file paths in a zip that already exist at `dest`.
fn check_zip_conflicts(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    dest: &Path,
    strip_root: bool,
) -> Vec<String> {
    let mut conflicts = Vec::new();
    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            let raw_name = entry.name().to_string();
            let name = if strip_root {
                match strip_root_component(&raw_name) {
                    Some(n) => n.to_string(),
                    None => continue,
                }
            } else {
                raw_name
            };
            if !name.ends_with('/') && dest.join(&name).exists() {
                conflicts.push(name);
            }
        }
    }
    conflicts
}

/// Extract a zip archive into `dest`. If `skip_existing` is true, files that
/// already exist at the destination are left untouched. When `strip_root` is
/// true, the top-level directory in the archive is stripped before extracting.
/// Returns the number of files written.
fn extract_zip_to(
    archive: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    dest: &Path,
    skip_existing: bool,
    strip_root: bool,
) -> Result<u64> {
    let mut count = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let raw_name = entry.name().to_string();

        let name = if strip_root {
            match strip_root_component(&raw_name) {
                Some(n) => n.to_string(),
                None => continue,
            }
        } else {
            raw_name
        };

        let outpath = dest.join(&name);

        if name.ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if skip_existing && outpath.exists() {
                continue;
            }
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut entry, &mut outfile)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Download a zip from `url` and return the bytes.
fn download_zip(url: &str) -> Result<Vec<u8>> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::default_spinner().template("{spinner:.green} {msg}")?);
    pb.set_message(format!("Downloading {url}..."));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let response = ureq::get(url)
        .call()
        .with_context(|| format!("Failed to download {url}"))?;
    let bytes = response
        .into_body()
        .with_config()
        .limit(512 * 1024 * 1024) // 512 MiB — Godot downloads are ~60-100 MiB
        .read_to_vec()
        .with_context(|| format!("Failed to read response from {url}"))?;
    pb.finish_with_message("Download complete");
    Ok(bytes)
}

// =============================================================================
// Commands
// =============================================================================

fn cmd_init(
    godot_version: &str,
    skill_target: &str,
    assets: &[powertool_common::config::Asset],
) -> Result<()> {
    println!("=== First-time project setup ===\n");
    println!("This will download Godot, fetch all assets, and configure tooling.");
    println!("Existing files in the project may be overwritten.\n");

    if !confirm_prompt("Continue?")? {
        println!("Aborted.");
        return Ok(());
    }

    let root = project_root()?;
    let tools = tools_dir()?;
    fs::create_dir_all(&tools)?;

    // --- 1. Godot engine ---
    download_godot(godot_version, &tools)?;

    // --- 2. Copy repo addons into project ---
    copy_addons(&root)?;

    // --- 3. Assets (e.g. GUT) — before import so addons are present ---
    let dest = godot_dir()?;
    for asset in assets {
        println!("\nFetching asset: {}", asset.url);
        let bytes = download_zip(&asset.url)?;
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;
        let count = extract_zip_to(&mut archive, &dest, false, asset.strip_root)?;
        println!("Extracted {count} files");
    }

    // --- 4. Build GDExtension (only if enabled in workspace) ---
    if extension_enabled() {
        println!("\nBuilding GDExtension...");
        cmd_build(false, false, false)?;
    }

    // --- 4. Initialize Godot project ---
    println!("\nInitializing Godot project...");
    let godot = godot_binary()?;
    let godot_path = godot_dir()?;

    let status = Command::new(&godot)
        .args(["--headless", "--import"])
        .current_dir(&godot_path)
        .status()
        .context("Failed to initialize Godot project")?;

    if !status.success() {
        bail!("Godot project initialization failed");
    }

    // --- 5. Generate API docs ---
    println!("\n=== Generating Godot API docs ===\n");
    let status = Command::new("cargo")
        .args(["run", "-p", "powertool-docs", "--release", "--", "generate"])
        .current_dir(&root)
        .status()
        .context("Failed to run doc generator")?;

    if !status.success() {
        eprintln!("Warning: Doc generation failed. You can retry with:");
        eprintln!("  cargo run -p powertool-docs -- generate");
    }

    // --- 6. Install skill files ---
    println!("\n=== Installing skill files ===\n");
    match cmd_skill_install(skill_target) {
        Ok(()) => {},
        Err(e) => {
            eprintln!("Warning: Skill install failed: {e}");
            eprintln!("  You can retry with: cargo xtask skill install");
        }
    }

    // --- 7. Build MCP server ---
    println!("\n=== Building MCP server ===\n");
    let status = Command::new("cargo")
        .args(["build", "-p", "powertool-mcp", "--release"])
        .current_dir(&root)
        .status()
        .context("Failed to build MCP server")?;

    if status.success() {
        let mcp_bin = root.join("target").join("release").join("powertool-mcp");
        println!("MCP server built: {}", mcp_bin.display());
    } else {
        eprintln!("Warning: MCP server build failed. You can retry with:");
        eprintln!("  cargo build -p powertool-mcp --release");
    }

    // --- 8. Build LSP bridge ---
    println!("\n=== Building LSP bridge ===\n");
    let status = Command::new("cargo")
        .args(["build", "-p", "powertool-lsp-bridge", "--release"])
        .current_dir(&root)
        .status()
        .context("Failed to build LSP bridge")?;

    if status.success() {
        let lsp_bin = root.join("target").join("release").join("powertool-lsp-bridge");
        println!("LSP bridge built: {}", lsp_bin.display());
    } else {
        eprintln!("Warning: LSP bridge build failed. You can retry with:");
        eprintln!("  cargo build -p powertool-lsp-bridge --release");
    }

    // --- Done ---
    println!("\n=== Init complete! ===\n");
    println!("  cargo xtask editor         # Open Godot editor");
    println!("  cargo xtask run            # Run the game");
    println!("  cargo xtask test           # Run all tests");

    let mcp_bin = root.join("target").join("release").join("powertool-mcp");
    println!(
        "  claude mcp add godot -- {}  # Add MCP server to Claude Code",
        mcp_bin.display()
    );
    println!("  cargo xtask lsp-bridge install  # Configure GDScript LSP");

    Ok(())
}

fn cmd_setup(opts: &SetupOptions) -> Result<()> {
    println!("Setting up project tooling...\n");

    let root = project_root()?;
    let tools = tools_dir()?;
    fs::create_dir_all(&tools)?;

    // --- 1. Godot engine (version-aware) ---
    if !opts.skip_godot {
        download_godot(&opts.godot_version, &tools)?;
    } else {
        println!("Skipping Godot download (--skip-godot)");
    }

    // --- 2. Copy repo addons into project ---
    copy_addons(&root)?;

    // --- 3. Build GDExtension (only if enabled in workspace) ---
    if extension_enabled() {
        println!("\nBuilding GDExtension...");
        cmd_build(false, false, false)?;
    }

    // --- 3. Initialize Godot project ---
    println!("\nInitializing Godot project...");
    let godot = godot_binary()?;
    let godot_path = godot_dir()?;

    let status = Command::new(&godot)
        .args(["--headless", "--import"])
        .current_dir(&godot_path)
        .status()
        .context("Failed to initialize Godot project")?;

    if !status.success() {
        bail!("Godot project initialization failed");
    }

    // --- 4. Generate API docs ---
    if !opts.skip_docs {
        println!("\n=== Generating Godot API docs ===\n");
        let status = Command::new("cargo")
            .args(["run", "-p", "powertool-docs", "--release", "--", "generate"])
            .current_dir(&root)
            .status()
            .context("Failed to run doc generator")?;

        if !status.success() {
            eprintln!("Warning: Doc generation failed. You can retry with:");
            eprintln!("  cargo run -p powertool-docs -- generate");
        }
    } else {
        println!("\nSkipping API doc generation (--skip-docs)");
    }

    // --- 5. Install skill files ---
    if !opts.skip_skill {
        println!("\n=== Installing skill files ===\n");
        match cmd_skill_install(&opts.skill_target) {
            Ok(()) => {},
            Err(e) => {
                eprintln!("Warning: Skill install failed: {e}");
                eprintln!("  You can retry with: cargo xtask skill install");
            }
        }
    } else {
        println!("\nSkipping skill install (--skip-skill)");
    }

    // --- 6. Build MCP server ---
    if !opts.skip_mcp {
        println!("\n=== Building MCP server ===\n");
        let status = Command::new("cargo")
            .args(["build", "-p", "powertool-mcp", "--release"])
            .current_dir(&root)
            .status()
            .context("Failed to build MCP server")?;

        if status.success() {
            let mcp_bin = root.join("target").join("release").join("powertool-mcp");
            println!("MCP server built: {}", mcp_bin.display());
        } else {
            eprintln!("Warning: MCP server build failed. You can retry with:");
            eprintln!("  cargo build -p powertool-mcp --release");
        }
    } else {
        println!("\nSkipping MCP server build (--skip-mcp)");
    }

    // --- 7. Build LSP bridge ---
    if !opts.skip_mcp {
        println!("\n=== Building LSP bridge ===\n");
        let status = Command::new("cargo")
            .args(["build", "-p", "powertool-lsp-bridge", "--release"])
            .current_dir(&root)
            .status()
            .context("Failed to build LSP bridge")?;

        if status.success() {
            let lsp_bin = root.join("target").join("release").join("powertool-lsp-bridge");
            println!("LSP bridge built: {}", lsp_bin.display());
        } else {
            eprintln!("Warning: LSP bridge build failed. You can retry with:");
            eprintln!("  cargo build -p powertool-lsp-bridge --release");
        }
    }

    // --- Done ---
    println!("\n=== Setup complete! ===\n");
    println!("  cargo xtask editor         # Open Godot editor");
    println!("  cargo xtask run            # Run the game");
    println!("  cargo xtask test           # Run all tests");

    if !opts.skip_mcp {
        let mcp_bin = root.join("target").join("release").join("powertool-mcp");
        println!(
            "  claude mcp add godot -- {}  # Add MCP server to Claude Code",
            mcp_bin.display()
        );
        println!("  cargo xtask lsp-bridge install  # Configure GDScript LSP");
    }

    Ok(())
}

fn cmd_update(assets: &[powertool_common::config::Asset]) -> Result<()> {
    if assets.is_empty() {
        println!("No assets configured in template.toml");
        return Ok(());
    }

    let dest = godot_dir()?;

    for asset in assets {
        println!("\nFetching asset: {}", asset.url);
        let bytes = download_zip(&asset.url)?;
        let cursor = std::io::Cursor::new(bytes.clone());
        let mut archive = zip::ZipArchive::new(cursor)?;

        let strip = asset.strip_root;

        let mut total_files = 0usize;
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let raw = entry.name().to_string();
                let name = if strip {
                    match strip_root_component(&raw) {
                        Some(n) => n.to_string(),
                        None => continue,
                    }
                } else {
                    raw
                };
                if !name.ends_with('/') {
                    total_files += 1;
                }
            }
        }
        let conflicts = check_zip_conflicts(&mut archive, &dest, strip);

        if conflicts.is_empty() {
            println!("{total_files} files, no conflicts — extracting...");
            let cursor = std::io::Cursor::new(bytes);
            let mut archive = zip::ZipArchive::new(cursor)?;
            let count = extract_zip_to(&mut archive, &dest, false, strip)?;
            println!("Extracted {count} files");
        } else {
            println!("{total_files} files, {} conflict(s) with existing files:", conflicts.len());
            for (i, path) in conflicts.iter().enumerate() {
                if i >= 10 {
                    println!("  ... and {} more", conflicts.len() - 10);
                    break;
                }
                println!("  {path}");
            }

            let choice = confirm_prompt_yns("Overwrite all (y), cancel (n), or extract only new files (s)?")?;
            match choice {
                'y' => {
                    let cursor = std::io::Cursor::new(bytes);
                    let mut archive = zip::ZipArchive::new(cursor)?;
                    let count = extract_zip_to(&mut archive, &dest, false, strip)?;
                    println!("Extracted {count} files (overwrote conflicts)");
                },
                's' => {
                    let cursor = std::io::Cursor::new(bytes);
                    let mut archive = zip::ZipArchive::new(cursor)?;
                    let count = extract_zip_to(&mut archive, &dest, true, strip)?;
                    println!("Extracted {count} new files (skipped existing)");
                },
                _ => {
                    println!("Skipped.");
                },
            }
        }
    }

    println!("\n=== Update complete! ===");
    Ok(())
}

fn download_godot(version: &str, tools_dir: &Path) -> Result<()> {
    let godot_dir = tools_dir.join("godot");
    let stamp_path = godot_dir.join(".version");

    // Version-aware skip: only skip if the installed version matches
    if let Ok(installed) = fs::read_to_string(&stamp_path) {
        if installed.trim() == version {
            println!("Godot {version} already installed at {}", godot_dir.display());
            return Ok(());
        }
        println!("Godot version changed ({} -> {version}), re-downloading...", installed.trim());
        fs::remove_dir_all(&godot_dir)?;
    }

    println!("Downloading Godot {version}...");

    let (url, binary_name) = godot_download_info(version);

    let bytes = download_zip(&url)?;

    println!("Extracting Godot...");
    fs::create_dir_all(&godot_dir)?;

    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = godot_dir.join(file.name());

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                fs::create_dir_all(p)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    let extracted = godot_dir.join(&binary_name);
    let final_path = godot_dir.join("godot");

    if extracted.exists() {
        fs::rename(&extracted, &final_path)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&final_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&final_path, perms)?;
    }

    // Write version stamp
    fs::write(&stamp_path, version)?;

    println!("Godot {version} installed to {}", final_path.display());
    Ok(())
}

fn cmd_build(release: bool, web: bool, threads: bool) -> Result<()> {
    if !extension_enabled() {
        bail!(
            "Rust extension is not enabled.\n\
             To enable it, add \"extension\" to the workspace members in Cargo.toml.\n\
             See \"Enabling the Rust GDExtension\" in README.md for details."
        );
    }

    let mode = if release { "release" } else { "debug" };

    if web {
        cmd_build_wasm(release, threads)
    } else {
        cmd_build_native(release, mode)
    }
}

fn cmd_build_native(release: bool, mode: &str) -> Result<()> {
    println!("Building GDExtension ({mode})...");

    let root = project_root()?;
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("-p").arg("extension");

    if release {
        cmd.arg("--release");
    }

    let status = cmd
        .current_dir(&root)
        .status()
        .context("Failed to run cargo build")?;

    if !status.success() {
        bail!("Build failed");
    }

    let target_dir = root.join("target").join(mode);
    let lib_src = target_dir.join(extension_lib_name());
    let lib_dst = extension_bin_dir()?.join(extension_lib_name());

    if lib_src.exists() {
        fs::create_dir_all(lib_dst.parent().unwrap())?;
        fs::copy(&lib_src, &lib_dst).context("Failed to copy library to Godot addons")?;
        println!("Copied {} to {}", lib_src.display(), lib_dst.display());
    }

    println!("Build complete!");
    Ok(())
}

const WASM_RUSTFLAGS_BASE: &str = concat!(
    "-C link-args=-sSIDE_MODULE=2 ",
    "-Z link-native-libraries=no ",
    "-C llvm-args=-enable-emscripten-cxx-exceptions=0 ",
    "-Z emscripten-wasm-eh=false ",
    "-Z default-visibility=hidden",
);

const WASM_RUSTFLAGS_THREADS: &str = concat!(
    "-C link-args=-pthread ",
    "-C target-feature=+atomics",
);

fn cmd_build_wasm(release: bool, threads: bool) -> Result<()> {
    let mode = if release { "release" } else { "debug" };
    println!("Building GDExtension for WASM ({mode})...\n");

    let root = project_root()?;

    let emcc_paths = [
        "/usr/lib/emscripten",
        "/usr/local/lib/emscripten",
        "/opt/emscripten",
    ];

    let emcc_path = emcc_paths
        .iter()
        .find(|p| Path::new(p).join("emcc").exists())
        .map(PathBuf::from);

    let emcc_dir = emcc_path.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Emscripten not found. Please install emscripten:\n\
             - Arch Linux: pacman -S emscripten\n\
             - Other: https://emscripten.org/docs/getting_started/downloads.html"
        )
    })?;

    println!("Using emscripten from: {}", emcc_dir.display());

    println!("Checking Rust nightly toolchain...");
    let nightly_check = Command::new("rustup")
        .args(["run", "nightly", "rustc", "--version"])
        .output();

    if nightly_check.is_err() || !nightly_check.as_ref().unwrap().status.success() {
        bail!(
            "Rust nightly toolchain required for WASM builds.\n\
             Install with: rustup toolchain install nightly\n\
             Also needed:  rustup component add rust-src --toolchain nightly"
        );
    }

    println!("Ensuring rust-src component is available...");
    let _ = Command::new("rustup")
        .args(["component", "add", "rust-src", "--toolchain", "nightly"])
        .status();

    let current_path = env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", emcc_dir.display(), current_path);

    let godot_bin = godot_binary().ok();
    if let Some(ref bin) = godot_bin {
        println!("Using Godot binary for API generation: {}", bin.display());
    } else {
        println!("Warning: Godot binary not found. api-custom feature may fail.");
        println!("Run 'cargo xtask setup' first to download Godot.");
    }

    let web_bin_dir = godot_dir()?
        .join("addons")
        .join("extension")
        .join("bin")
        .join("web");
    fs::create_dir_all(&web_bin_dir)?;

    let target_dir = root
        .join("target")
        .join("wasm32-unknown-emscripten")
        .join(mode);

    let run_wasm_build = |extra_flags: &str,
                          extra_args: &[&str],
                          dest_name: &str|
     -> Result<()> {
        let rustflags = if extra_flags.is_empty() {
            WASM_RUSTFLAGS_BASE.to_string()
        } else {
            format!("{WASM_RUSTFLAGS_BASE} {extra_flags}")
        };

        let mut cmd = Command::new("cargo");
        cmd.arg("+nightly")
            .arg("build")
            .arg("-Zbuild-std")
            .arg("-p")
            .arg("extension")
            .arg("--target")
            .arg("wasm32-unknown-emscripten");

        for arg in extra_args {
            cmd.arg(arg);
        }
        if release {
            cmd.arg("--release");
        }

        cmd.env("RUSTFLAGS", &rustflags)
            .env("PATH", &new_path)
            .env("EMSCRIPTEN", emcc_dir);
        if let Some(ref bin) = godot_bin {
            cmd.env("GODOT4_BIN", bin);
        }
        cmd.current_dir(&root);

        println!("\nRunning: cargo +nightly build -Zbuild-std -p extension \\");
        println!(
            "  --target wasm32-unknown-emscripten{}",
            if release { " --release" } else { "" }
        );
        for arg in extra_args {
            println!("  {arg}");
        }
        println!("  RUSTFLAGS=\"{rustflags}\"");

        let status = cmd
            .status()
            .with_context(|| format!("Failed to run cargo build for WASM ({dest_name})"))?;

        if !status.success() {
            bail!("WASM build failed ({dest_name})");
        }

        let src = target_dir.join("extension.wasm");
        let dst = web_bin_dir.join(dest_name);
        if src.exists() {
            fs::copy(&src, &dst)
                .with_context(|| format!("Failed to copy {dest_name} to Godot addons"))?;
            println!("Copied -> {}", dst.display());
        } else {
            bail!(
                "Expected output not found: {}\nCheck that the crate name is 'extension'.",
                src.display()
            );
        }
        Ok(())
    };

    if threads {
        println!("--- WASM build 1/2: nothreads ---");
    }
    run_wasm_build("", &["--features", "nothreads"], "extension.wasm")?;

    if threads {
        println!("\n--- WASM build 2/2: threaded ---");
        run_wasm_build(WASM_RUSTFLAGS_THREADS, &[], "extension.threads.wasm")?;
    }

    println!("\nWASM build complete!");
    println!(
        "  Nothreads: {}",
        web_bin_dir.join("extension.wasm").display()
    );
    if threads {
        println!(
            "  Threaded:  {}",
            web_bin_dir.join("extension.threads.wasm").display()
        );
    }
    println!(
        "\nNote: export_presets.cfg thread_support must match the variant you use.\n\
         After changing thread_support, reload the Godot project before re-exporting."
    );
    Ok(())
}

fn cmd_run(editor: bool, godot_args: &[String]) -> Result<()> {
    if extension_enabled() {
        let lib_path = extension_bin_dir()?.join(extension_lib_name());
        if !lib_path.exists() {
            println!("GDExtension not found, building...");
            cmd_build(false, false, false)?;
        }
    }

    let godot = godot_binary()?;
    let godot_path = godot_dir()?;

    let mut cmd = Command::new(&godot);

    if editor {
        cmd.arg("--editor");
        println!("Opening Godot editor...");
    } else {
        println!("Running game...");
    }

    cmd.args(godot_args);
    cmd.current_dir(&godot_path);

    let status = cmd.status().context("Failed to run Godot")?;

    if !status.success() {
        bail!("Godot exited with error");
    }

    Ok(())
}

fn cmd_test(rust_only: bool, godot_only: bool, verbose: bool, filter: Option<&str>) -> Result<()> {
    let mut any_failed = false;

    if !godot_only {
        println!("Running Rust tests...\n");
        let root = project_root()?;

        let mut cmd = Command::new("cargo");
        cmd.arg("test").arg("--workspace");

        if let Some(f) = filter {
            cmd.arg("--").arg(f);
        }

        let status = cmd
            .current_dir(&root)
            .status()
            .context("Failed to run Rust tests")?;

        if !status.success() {
            any_failed = true;
            eprintln!("\nRust tests failed!");
        } else {
            println!("\nRust tests passed!");
        }
    }

    if !rust_only {
        println!("\nRunning Godot GUT tests...\n");

        if extension_enabled() {
            let lib_path = extension_bin_dir()?.join(extension_lib_name());
            if !lib_path.exists() {
                println!("GDExtension not found, building...");
                cmd_build(false, false, false)?;
            }
        }

        let godot = godot_binary()?;
        let godot_path = godot_dir()?;

        let mut cmd = Command::new(&godot);
        cmd.arg("--headless");
        cmd.args(["-s", "addons/gut/gut_cmdln.gd"]);
        cmd.args(["-gdir=res://tests/unit/", "-gdir=res://tests/integration/"]);
        cmd.arg("-ginclude_subdirs");
        cmd.arg("-gexit");

        if verbose {
            cmd.arg("-glog=2");
        }

        if let Some(f) = filter {
            cmd.arg(format!("-gunit_test_name={f}"));
        }

        cmd.current_dir(&godot_path);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to run GUT tests")?;

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                println!("{line}");
            }
        }

        let status = child.wait()?;

        if !status.success() {
            any_failed = true;
            eprintln!("\nGUT tests failed!");
        } else {
            println!("\nGUT tests passed!");
        }
    }

    if any_failed {
        bail!("Some tests failed");
    }

    println!("\nAll tests passed!");
    Ok(())
}

fn cmd_clean(all: bool) -> Result<()> {
    let root = project_root()?;

    println!("Cleaning Cargo build artifacts...");
    Command::new("cargo")
        .arg("clean")
        .current_dir(&root)
        .status()
        .context("Failed to run cargo clean")?;

    let bin_dir = extension_bin_dir()?;
    if bin_dir.exists() {
        println!("Removing extension binaries...");
        fs::remove_dir_all(&bin_dir)?;
    }

    if all {
        let tools = tools_dir()?;
        if tools.exists() {
            println!("Removing tools directory...");
            fs::remove_dir_all(&tools)?;
        }
    }

    println!("Clean complete!");
    Ok(())
}

fn cmd_doctor() -> Result<()> {
    println!("Checking project health...\n");

    let mut all_ok = true;

    print!("Rust toolchain: ");
    if which::which("cargo").is_ok() {
        let output = Command::new("rustc").arg("--version").output()?;
        let version = String::from_utf8_lossy(&output.stdout);
        println!("\x1b[32m✓\x1b[0m {}", version.trim());
    } else {
        println!("\x1b[31m✗\x1b[0m not found");
        all_ok = false;
    }

    print!("Rust nightly:   ");
    let nightly_check = Command::new("rustup")
        .args(["run", "nightly", "rustc", "--version"])
        .output();
    match nightly_check {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            println!("\x1b[32m✓\x1b[0m {}", version.trim());
        },
        _ => {
            println!("\x1b[33m-\x1b[0m not installed (optional, for WASM builds)");
        },
    }

    print!("Godot:          ");
    match godot_binary() {
        Ok(path) => {
            let output = Command::new(&path).arg("--version").output()?;
            let version = String::from_utf8_lossy(&output.stdout);
            println!("\x1b[32m✓\x1b[0m {} ({})", version.trim(), path.display());
        },
        Err(_) => {
            println!("\x1b[31m✗\x1b[0m not found - run 'cargo xtask setup'");
            all_ok = false;
        },
    }

    print!("GUT:            ");
    let gut_dir = godot_dir()?.join("addons").join("gut");
    if gut_dir.exists() {
        println!("\x1b[32m✓\x1b[0m installed");
    } else {
        println!("\x1b[31m✗\x1b[0m not installed - run 'cargo xtask setup'");
        all_ok = false;
    }

    print!("GDExtension:    ");
    if extension_enabled() {
        let lib_path = extension_bin_dir()?.join(extension_lib_name());
        if lib_path.exists() {
            println!("\x1b[32m✓\x1b[0m enabled and built");
        } else {
            println!("\x1b[33m-\x1b[0m enabled but not built - run 'cargo xtask build'");
        }
    } else {
        println!("\x1b[33m-\x1b[0m disabled (see README.md to enable)");
    }

    print!("Godot project:  ");
    let project_file = godot_dir()?.join("project.godot");
    if project_file.exists() {
        println!("\x1b[32m✓\x1b[0m found");
    } else {
        println!("\x1b[31m✗\x1b[0m project.godot missing");
        all_ok = false;
    }

    print!("Emscripten:     ");
    let emcc_paths = [
        "/usr/lib/emscripten",
        "/usr/local/lib/emscripten",
        "/opt/emscripten",
    ];
    let emcc_found = emcc_paths
        .iter()
        .any(|p| Path::new(p).join("emcc").exists());
    if emcc_found {
        println!("\x1b[32m✓\x1b[0m installed");
    } else {
        println!("\x1b[33m-\x1b[0m not installed (optional, for WASM builds)");
    }

    println!();

    if all_ok {
        println!("\x1b[32mAll required checks passed!\x1b[0m");
    } else {
        println!("\x1b[31mSome checks failed.\x1b[0m Run 'cargo xtask setup' to fix.");
    }

    Ok(())
}

fn cmd_fmt(check: bool) -> Result<()> {
    println!("Formatting Rust code...");

    let root = project_root()?;
    let mut cmd = Command::new("cargo");
    cmd.arg("fmt");

    if check {
        cmd.arg("--check");
    }

    let status = cmd
        .current_dir(&root)
        .status()
        .context("Failed to run cargo fmt")?;

    if !status.success() {
        if check {
            bail!("Formatting check failed");
        } else {
            bail!("Formatting failed");
        }
    }

    println!("Formatting complete!");
    Ok(())
}

fn cmd_lint() -> Result<()> {
    println!("Running Rust linter (clippy)...\n");

    let root = project_root()?;

    let status = Command::new("cargo")
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(&root)
        .status()
        .context("Failed to run clippy")?;

    if !status.success() {
        bail!("Linting failed");
    }

    println!("\nLinting complete!");
    Ok(())
}

fn cmd_ci() -> Result<()> {
    println!("Running CI pipeline...\n");

    println!("=== Step 1/3: Format Check ===\n");
    cmd_fmt(true)?;

    println!("\n=== Step 2/3: Lint ===\n");
    cmd_lint()?;

    println!("\n=== Step 3/3: Rust Tests ===\n");
    cmd_test(true, false, false, None)?;

    println!("\n=== CI Pipeline Complete ===");
    Ok(())
}

fn cmd_docs(no_open: bool) -> Result<()> {
    println!("Generating documentation...\n");

    let root = project_root()?;

    let status = Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(&root)
        .status()
        .context("Failed to generate documentation")?;

    if !status.success() {
        bail!("Documentation generation failed");
    }

    println!("\nDocumentation generated!");

    if !no_open {
        let doc_path = root
            .join("target")
            .join("doc")
            .join("powertool_common")
            .join("index.html");
        if doc_path.exists() {
            println!("Opening documentation in browser...");
            let _ = open_path(&doc_path);
        }
    }

    Ok(())
}

fn cmd_editor() -> Result<()> {
    let godot = godot_binary()?;
    let godot_path = godot_dir()?;

    println!("Opening Godot editor...");

    let status = Command::new(&godot)
        .arg("--editor")
        .current_dir(&godot_path)
        .status()
        .context("Failed to open Godot editor")?;

    if !status.success() {
        bail!("Godot editor exited with error");
    }

    Ok(())
}

fn cmd_dev(release: bool) -> Result<()> {
    println!("Building and running...\n");
    cmd_build(release, false, false)?;
    println!();
    cmd_run(false, &[])
}

// =============================================================================
// Web Commands
// =============================================================================

fn web_dir() -> Result<PathBuf> {
    Ok(project_root()?.join("web"))
}

fn npm_cmd() -> Result<PathBuf> {
    which::which("npm").context("npm not found. Please install Node.js from https://nodejs.org/")
}

fn cmd_web_setup() -> Result<()> {
    println!("Web frontend setup instructions:\n");

    let web = web_dir()?;

    print!("Node.js: ");
    match which::which("node") {
        Ok(_) => {
            let output = Command::new("node").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        },
        Err(_) => println!("NOT FOUND — install from https://nodejs.org/"),
    }

    print!("npm: ");
    match which::which("npm") {
        Ok(_) => {
            let output = Command::new("npm").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        },
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

fn cmd_web_dev(port: u16) -> Result<()> {
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

fn cmd_web_build() -> Result<()> {
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

const GODOT_EXPORT_EXTENSIONS: &[&str] = &[
    "html", "js", "wasm", "pck", "png", "json", "worker.js",
];

fn is_godot_export_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    name.starts_with("godot.")
        || name == "extension.wasm"
        || name == "extension.threads.wasm"
        || GODOT_EXPORT_EXTENSIONS
            .iter()
            .any(|ext| name.ends_with(ext) && name.starts_with("godot"))
}

fn cmd_web_export(release: bool) -> Result<()> {
    println!("Exporting Godot project for web...\n");

    let godot_path = godot_dir()?;
    let web = web_dir()?;
    let export_dir = web.join("public");

    println!("Cleaning previous Godot export artifacts...");
    if export_dir.exists() {
        for entry in fs::read_dir(&export_dir)? {
            let path = entry?.path();
            if path.is_file() && is_godot_export_file(&path) {
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
    cmd_build(release, false, false)?;

    println!("\nStep 2: Building WASM GDExtension...");
    match cmd_build(release, true, false) {
        Ok(()) => println!("WASM extension built successfully!"),
        Err(e) => {
            println!("Warning: WASM build failed: {e}");
            println!("Continuing with export (GDScript will work, Rust extension will not)...\n");
        },
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

fn cmd_web_test(headed: bool, ui: bool, debug: bool, filter: Option<&str>) -> Result<()> {
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

fn cmd_web_preview(port: u16) -> Result<()> {
    let web = web_dir()?;
    let npm = npm_cmd()?;

    if !web.join("dist").exists() {
        println!("Production build not found. Building first...\n");
        cmd_web_build()?;
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

fn cmd_web_doctor() -> Result<()> {
    println!("Checking web project health...\n");

    let web = web_dir()?;
    let mut issues = Vec::new();

    print!("Node.js: ");
    match which::which("node") {
        Ok(_) => {
            let output = Command::new("node").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        },
        Err(_) => {
            println!("NOT FOUND");
            issues.push("Node.js not installed. Visit https://nodejs.org/");
        },
    }

    print!("npm: ");
    match which::which("npm") {
        Ok(_) => {
            let output = Command::new("npm").arg("--version").output()?;
            println!("{}", String::from_utf8_lossy(&output.stdout).trim());
        },
        Err(_) => {
            println!("NOT FOUND");
            issues.push("npm not installed");
        },
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
        },
        _ => {
            println!("NOT INSTALLED");
            issues.push("Run 'cargo xtask web setup' to install Playwright");
        },
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

// =============================================================================
// Skill Commands
// =============================================================================

fn skill_source_dir() -> Result<PathBuf> {
    Ok(project_root()?.join("skill"))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn cmd_skill_install(target: &str) -> Result<()> {
    let root = project_root()?;
    let skill_target = SkillTarget::from_str_or_path(target);
    let dest = skill_target.skill_dir(&root).join("godot-powertool");
    let source = skill_source_dir()?;

    if !source.exists() {
        bail!("Skill source directory not found at {}", source.display());
    }

    println!("Installing skill files to {}...", dest.display());

    // Copy skill content
    copy_dir_recursive(&source, &dest)?;

    // Generate doc_api if not present in destination
    let doc_api_dest = dest.join("doc_api");
    let doc_api_source = root.join("doc_api");
    if doc_api_source.exists() && doc_api_source.join("_common.md").exists() {
        println!("Copying API docs...");
        copy_dir_recursive(&doc_api_source, &doc_api_dest)?;
    } else {
        println!(
            "Note: API docs not generated yet. Run 'cargo run -p powertool-docs -- generate' first,\n\
             then 'cargo xtask skill update' to include them."
        );
    }

    println!("Skill installed to {}", dest.display());
    Ok(())
}

fn cmd_skill_update(target: &str) -> Result<()> {
    let root = project_root()?;
    let skill_target = SkillTarget::from_str_or_path(target);
    let dest = skill_target.skill_dir(&root).join("godot-powertool");

    if dest.exists() {
        println!("Removing existing skill files...");
        fs::remove_dir_all(&dest)?;
    }

    cmd_skill_install(target)
}

fn cmd_skill_remove(target: &str) -> Result<()> {
    let root = project_root()?;
    let skill_target = SkillTarget::from_str_or_path(target);
    let dest = skill_target.skill_dir(&root).join("godot-powertool");

    if dest.exists() {
        println!("Removing skill files from {}...", dest.display());
        fs::remove_dir_all(&dest)?;
        println!("Skill removed.");
    } else {
        println!("No skill files found at {}", dest.display());
    }

    Ok(())
}

// =============================================================================
// MCP Commands
// =============================================================================

fn cmd_mcp_run() -> Result<()> {
    let root = project_root()?;
    let mcp_bin = root.join("target").join("release").join("powertool-mcp");

    if !mcp_bin.exists() {
        println!("MCP server not built. Building in release mode...");
        let status = Command::new("cargo")
            .args(["build", "-p", "powertool-mcp", "--release"])
            .current_dir(&root)
            .status()
            .context("Failed to build MCP server")?;
        if !status.success() {
            bail!("Failed to build MCP server");
        }
    }

    let status = Command::new(&mcp_bin)
        .status()
        .context("Failed to run MCP server")?;

    if !status.success() {
        bail!("MCP server exited with error");
    }
    Ok(())
}

fn cmd_mcp_install(client: &str) -> Result<()> {
    let root = project_root()?;
    let mcp_bin = root.join("target").join("release").join("powertool-mcp");
    let mcp_path = mcp_bin.to_string_lossy();

    match client {
        "claude" => {
            println!("Add to Claude Code with:");
            println!("  claude mcp add godot -- {mcp_path}");
            println!();
            println!("Or build first: cargo build -p powertool-mcp --release");
        }
        "cursor" => {
            println!("Add to .cursor/mcp.json:");
            println!(
                r#"{{
  "mcpServers": {{
    "godot": {{
      "command": "{mcp_path}"
    }}
  }}
}}"#
            );
        }
        _ => {
            println!("Generic MCP server configuration:");
            println!("  Command: {mcp_path}");
            println!("  Transport: stdio");
            println!("  Build first: cargo build -p powertool-mcp --release");
        }
    }

    Ok(())
}

// =============================================================================
// LSP Bridge Commands
// =============================================================================

fn cmd_lsp_bridge_run() -> Result<()> {
    let root = project_root()?;
    let bin = root.join("target").join("release").join("powertool-lsp-bridge");

    if !bin.exists() {
        println!("LSP bridge not built. Building in release mode...");
        let status = Command::new("cargo")
            .args(["build", "-p", "powertool-lsp-bridge", "--release"])
            .current_dir(&root)
            .status()
            .context("Failed to build LSP bridge")?;
        if !status.success() {
            bail!("Failed to build LSP bridge");
        }
    }

    let status = Command::new(&bin)
        .status()
        .context("Failed to run LSP bridge")?;

    if !status.success() {
        bail!("LSP bridge exited with error");
    }
    Ok(())
}

fn cmd_lsp_bridge_install(client: &str) -> Result<()> {
    let root = project_root()?;
    let bin = root.join("target").join("release").join("powertool-lsp-bridge");
    let bin_path = bin.to_string_lossy();

    match client {
        "claude" => {
            println!("Add GDScript LSP to your project's .claude/settings.json:\n");
            println!(r#"{{
  "lspServers": {{
    "gdscript": {{
      "command": "{bin_path}",
      "args": [],
      "extensionToLanguage": {{
        ".gd": "gdscript"
      }},
      "restartOnCrash": true,
      "maxRestarts": 3
    }}
  }}
}}"#);
            println!();
            println!("Build first if needed: cargo build -p powertool-lsp-bridge --release");
            println!("Requires Godot editor running with LSP enabled (port 6005).");
        }
        _ => {
            println!("GDScript LSP bridge configuration:");
            println!("  Command: {bin_path}");
            println!("  Transport: stdio");
            println!("  Language: gdscript");
            println!("  Build first: cargo build -p powertool-lsp-bridge --release");
            println!("  Requires Godot editor running with LSP enabled (port 6005).");
        }
    }

    Ok(())
}
