use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct TemplateConfig {
    #[serde(default = "default_godot")]
    pub godot: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub extension: ExtensionConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub url: String,
    /// Strip the top-level directory from the zip before extracting.
    /// Useful for GitHub release zips that wrap everything in a `Name-version/` folder.
    #[serde(default)]
    pub strip_root: bool,
}

/// GDExtension layout: where the Rust crate lives, where its build artifacts
/// land in the Godot project, and the basename used for the platform-specific
/// library filename (`lib{basename}.so`, `{basename}.dll`, `lib{basename}.dylib`).
///
/// Defaults preserve the bundled `sample_extension` demo so existing
/// powertool-derived projects work unchanged without adding the block.
#[derive(Debug, Deserialize, Clone)]
pub struct ExtensionConfig {
    #[serde(default = "default_crate_name")]
    pub crate_name: String,
    #[serde(default = "default_crate_path")]
    pub crate_path: String,
    #[serde(default = "default_deploy_path")]
    pub deploy_path: String,
    #[serde(default = "default_lib_basename")]
    pub lib_basename: String,
}

impl Default for ExtensionConfig {
    fn default() -> Self {
        Self {
            crate_name: default_crate_name(),
            crate_path: default_crate_path(),
            deploy_path: default_deploy_path(),
            lib_basename: default_lib_basename(),
        }
    }
}

fn default_godot() -> String {
    "4.6.1".to_string()
}

fn default_crate_name() -> String {
    "sample_extension".to_string()
}

fn default_crate_path() -> String {
    "addons/sample_extension/rust".to_string()
}

fn default_deploy_path() -> String {
    "godot/addons/sample_extension".to_string()
}

fn default_lib_basename() -> String {
    "sample_extension".to_string()
}

/// Load `template.toml` from a project root directory.
pub fn load_template_config(root: &Path) -> Result<TemplateConfig> {
    let path = root.join("template.toml");
    if path.exists() {
        let contents =
            fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
        toml::from_str(&contents)
            .with_context(|| format!("Failed to parse {}", path.display()))
    } else {
        Ok(TemplateConfig::default())
    }
}
