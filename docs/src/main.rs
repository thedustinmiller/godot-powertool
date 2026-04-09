use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

mod bbcode;
mod bootstrap;
mod class_list;
mod converter;

#[derive(Parser)]
#[command(name = "powertool-docs")]
#[command(about = "Generate Godot API documentation from XML class references")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch Godot XML docs and generate markdown API reference
    Generate {
        /// Input directory containing Godot XML class files
        #[arg(short, long)]
        input: Option<PathBuf>,

        /// Output directory for per-class markdown files
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Use full descriptions instead of first-sentence summaries
        #[arg(long)]
        full: bool,

        /// Only convert these specific classes
        #[arg(long, num_args = 1..)]
        classes: Option<Vec<String>>,

        /// Godot version to fetch docs for (overrides template.toml)
        #[arg(long)]
        version: Option<String>,

        /// Skip fetching docs (use existing XML source)
        #[arg(long)]
        skip_fetch: bool,
    },

    /// Remove generated docs and source XML
    Clean,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate {
            input,
            output,
            full,
            classes,
            version,
            skip_fetch,
        } => cmd_generate(input, output, full, classes, version, skip_fetch),
        Commands::Clean => cmd_clean(),
    }
}

fn find_project_root() -> Result<PathBuf> {
    // Walk up from cwd looking for template.toml
    let cwd = std::env::current_dir()?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join("template.toml").exists() {
            return Ok(dir.to_path_buf());
        }
        dir = dir.parent().context("Could not find project root (no template.toml found)")?;
    }
}

fn cmd_generate(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    full: bool,
    classes: Option<Vec<String>>,
    version: Option<String>,
    skip_fetch: bool,
) -> Result<()> {
    let root = find_project_root()?;
    let doc_source = root.join("doc_source");
    let doc_api = output.unwrap_or_else(|| root.join("doc_api"));

    // Determine version
    let godot_version = if let Some(v) = version {
        v
    } else {
        let config = powertool_common::config::load_template_config(&root)?;
        config.godot.clone()
    };

    // Fetch XML docs if needed
    let xml_dir = if let Some(dir) = input {
        dir
    } else if !skip_fetch {
        bootstrap::fetch_godot_docs(&godot_version, &doc_source)?
    } else {
        let default_xml = doc_source.join("godot").join("doc").join("classes");
        if !default_xml.exists() {
            bail!("XML docs not found at {}. Run without --skip-fetch first.", default_xml.display());
        }
        default_xml
    };

    // Configure converter
    let config = if full {
        converter::ConversionConfig {
            class_description: converter::DescriptionMode::Full,
            method_descriptions: converter::DescriptionMode::FirstSentence,
            property_descriptions: converter::DescriptionMode::FirstSentence,
            ..Default::default()
        }
    } else {
        converter::ConversionConfig::default()
    };

    let classes_filter = classes.as_deref();

    converter::convert_directory_split(&xml_dir, &doc_api, &config, classes_filter)?;

    Ok(())
}

fn cmd_clean() -> Result<()> {
    let root = find_project_root()?;

    let doc_source = root.join("doc_source");
    if doc_source.exists() {
        println!("Removing {}...", doc_source.display());
        std::fs::remove_dir_all(&doc_source)?;
    }

    let doc_api = root.join("doc_api");
    if doc_api.exists() {
        println!("Removing {}...", doc_api.display());
        std::fs::remove_dir_all(&doc_api)?;
    }

    println!("Clean complete!");
    Ok(())
}
