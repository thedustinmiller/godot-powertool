use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct TemplateConfig {
    #[serde(default = "default_godot")]
    pub godot: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub url: String,
}

fn default_godot() -> String {
    "4.6.1".to_string()
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
