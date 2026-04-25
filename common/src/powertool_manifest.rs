//! `.powertool.toml` — provenance file written by `cargo xtask init` and
//! updated by `cargo xtask update`. The user owns `[source]` (where to pull
//! from + what ref to track); the updater owns `[applied]` (what was actually
//! synced and when, for diagnostics + drift detection).

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const MANIFEST_FILENAME: &str = ".powertool.toml";
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Paths under the project root that `cargo xtask update` mirrors verbatim
/// from the upstream powertool repo. Anything outside this list is user-owned
/// and the updater never touches it. Entries may be either files or
/// directories; directory entries are recursively mirrored, with any files
/// inside the directory that no longer exist upstream removed locally.
pub const MANAGED_PATHS: &[&str] = &[
    "addons/powertool",
    "addons/sample_extension",
    "xtask",
    "mcp",
    "common",
    "lsp-bridge",
    "godot_docs",
    "skill",
    "scripts",
    ".gitignore",
];

/// Paths whose upstream changes the updater warns about — they're user-owned
/// (so we never write to them) but a project-breaking diff in any of them
/// usually requires a manual fix-up after `update`.
pub const DRIFT_WATCH_PATHS: &[&str] = &["Cargo.toml", "README.md", "template.toml"];

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub schema_version: u32,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<Applied>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Source {
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Applied {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub commit: String,
    pub updated_at: String,
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(MANIFEST_FILENAME)
}

pub fn load(root: &Path) -> Result<Option<Manifest>> {
    let path = manifest_path(root);
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let manifest: Manifest = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(manifest))
}

pub fn save(root: &Path, manifest: &Manifest) -> Result<()> {
    let path = manifest_path(root);
    let header = "# Managed by `cargo xtask update`. Edit [source].url or [source].ref\n\
                  # to change where powertool is pulled from. The [applied] block is\n\
                  # rewritten by the updater after a successful sync — it's diagnostic\n\
                  # provenance, not configuration.\n\n";
    let body = toml::to_string_pretty(manifest).context("Failed to serialize manifest")?;
    fs::write(&path, format!("{header}{body}"))
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Current UTC timestamp formatted as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso8601(secs)
}

/// Convert Unix epoch seconds to `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Uses Howard Hinnant's days-from-civil algorithm (public domain) so we
/// don't pull in `chrono` or `time` for one timestamp string.
fn format_iso8601(epoch_secs: u64) -> String {
    let total_days = (epoch_secs / 86_400) as i64;
    let secs_of_day = epoch_secs % 86_400;
    let h = (secs_of_day / 3600) as u32;
    let m = ((secs_of_day % 3600) / 60) as u32;
    let sec = (secs_of_day % 60) as u32;

    let z = total_days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    if mo <= 2 {
        y += 1;
    }

    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{sec:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero() {
        assert_eq!(format_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_timestamp() {
        // 2026-04-25T14:32:00Z = 1777127520
        assert_eq!(format_iso8601(1_777_127_520), "2026-04-25T14:32:00Z");
    }

    #[test]
    fn leap_year_feb() {
        // 2024-02-29T00:00:00Z = 1709164800
        assert_eq!(format_iso8601(1_709_164_800), "2024-02-29T00:00:00Z");
    }
}
