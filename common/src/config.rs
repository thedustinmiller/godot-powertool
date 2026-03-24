use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct TemplateConfig {
    #[serde(default)]
    pub versions: Versions,
}

#[derive(Debug, Deserialize, Default)]
pub struct Versions {
    #[serde(default = "default_godot")]
    pub godot: String,
    #[serde(default = "default_gut")]
    pub gut: String,
    #[serde(default = "default_mcp")]
    pub mcp: String,
    #[serde(default = "default_skill")]
    pub skill: String,
    #[serde(default)]
    pub docs: Option<String>,
}

impl Versions {
    /// Returns the docs version, falling back to the godot version.
    pub fn docs_version(&self) -> &str {
        self.docs.as_deref().unwrap_or(&self.godot)
    }
}

fn default_godot() -> String {
    "4.6.1".to_string()
}
fn default_gut() -> String {
    "9.6.0".to_string()
}
fn default_mcp() -> String {
    "0.1.0".to_string()
}
fn default_skill() -> String {
    "0.1.0".to_string()
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
