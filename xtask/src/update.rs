//! `cargo xtask update` — sync managed paths from the upstream powertool repo
//! per `.powertool.toml`. See `common::powertool_manifest` for schema and
//! `MANAGED_PATHS` for the rsync set.

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use powertool_common::powertool_manifest::{
    self as manifest, Applied, CURRENT_SCHEMA_VERSION, DRIFT_WATCH_PATHS, MANAGED_PATHS,
    ManagedPathsConfig, Manifest, Source,
};
use walkdir::WalkDir;

pub struct UpdateOptions {
    /// Override the pinned ref for this run; on success, persisted back to
    /// `[source].ref` so subsequent updates track the new ref.
    pub override_ref: Option<String>,
    /// Print the rsync delta but don't write or delete anything.
    pub check_only: bool,
    /// Bypass the dirty-state guard for managed paths.
    pub force: bool,
    /// First-time setup: create `.powertool.toml` from this URL (and ref) and
    /// exit.
    pub bootstrap_url: Option<String>,
    pub bootstrap_ref: Option<String>,
}

#[derive(Default)]
struct SyncStats {
    written: u64,
    deleted: u64,
}

impl SyncStats {
    fn add(&mut self, other: SyncStats) {
        self.written += other.written;
        self.deleted += other.deleted;
    }
}

pub fn cmd_update(root: &Path, opts: UpdateOptions) -> Result<()> {
    // --- Bootstrap mode: write a fresh manifest and exit. --------------------
    if let Some(url) = opts.bootstrap_url {
        let path = manifest::manifest_path(root);
        if path.exists() {
            bail!(
                "{} already exists. Edit [source] in place, or use --ref to repin.",
                path.display()
            );
        }
        let m = Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            source: Source {
                url,
                git_ref: opts.bootstrap_ref.unwrap_or_else(|| "main".to_string()),
            },
            applied: None,
            managed_paths: ManagedPathsConfig::default(),
        };
        manifest::save(root, &m)?;
        println!(
            "Wrote {}. Run `cargo xtask update` to sync.",
            path.display()
        );
        return Ok(());
    }

    // --- Load manifest. -----------------------------------------------------
    let mut m = manifest::load(root)?.with_context(|| {
        format!(
            "{} not found. Bootstrap with: cargo xtask update --bootstrap <git-url> [--ref <r>]",
            manifest::manifest_path(root).display()
        )
    })?;
    if m.schema_version != CURRENT_SCHEMA_VERSION {
        bail!(
            "Unsupported manifest schema_version {} (expected {}).",
            m.schema_version,
            CURRENT_SCHEMA_VERSION
        );
    }

    // --- Verify git on PATH. ------------------------------------------------
    if which::which("git").is_err() {
        bail!("`git` not found on PATH. Install git, then re-run.");
    }

    let target_ref = opts
        .override_ref
        .clone()
        .unwrap_or_else(|| m.source.git_ref.clone());

    println!("Updating from {} @ {}", m.source.url, target_ref);

    // --- Effective managed-path set (after [managed_paths].exclude). --------
    for excl in &m.managed_paths.exclude {
        if !MANAGED_PATHS.iter().any(|p| p == excl) {
            eprintln!(
                "Warning: [managed_paths].exclude entry {excl:?} is not in MANAGED_PATHS \
                 — typo, or upstream removed/renamed it."
            );
        }
    }
    let managed: Vec<&str> = MANAGED_PATHS
        .iter()
        .copied()
        .filter(|p| !m.managed_paths.exclude.iter().any(|e| e == p))
        .collect();

    // --- Dirty-state guard. -------------------------------------------------
    if !opts.force {
        let dirty = list_dirty_managed_paths(root, &managed)?;
        if !dirty.is_empty() {
            eprintln!("Refusing to update — uncommitted changes in managed paths:");
            for p in &dirty {
                eprintln!("  {p}");
            }
            eprintln!("Commit/stash them, or re-run with --force to overwrite.");
            bail!("dirty managed paths");
        }
    }

    // --- Clone upstream into target/powertool-update/. ----------------------
    let tmp = root.join("target").join("powertool-update");
    if tmp.exists() {
        fs::remove_dir_all(&tmp).with_context(|| format!("Failed to clean {}", tmp.display()))?;
    }
    fs::create_dir_all(tmp.parent().unwrap())?;
    clone_at_ref(&m.source.url, &target_ref, &tmp)?;
    let upstream_commit = git_rev_parse(&tmp, "HEAD")?;

    // --- Drift detection (best-effort, only when we have a prior commit). ---
    let drift_changed = if let Some(prev) = m.applied.as_ref().map(|a| a.commit.clone()) {
        // Shallow clone may not contain `prev`. Try to deepen, then diff.
        let _ = Command::new("git")
            .args(["fetch", "--unshallow", "--quiet"])
            .current_dir(&tmp)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        diff_paths(&tmp, &prev, &upstream_commit, DRIFT_WATCH_PATHS).unwrap_or_default()
    } else {
        Vec::new()
    };

    // --- Sync (or simulate) each managed path. ------------------------------
    let mut total = SyncStats::default();
    for path in &managed {
        let stats = sync_one(&tmp.join(path), &root.join(path), opts.check_only)?;
        if stats.written > 0 || stats.deleted > 0 {
            println!("  {path:30}  +{} -{}", stats.written, stats.deleted);
        }
        total.add(stats);
    }

    if opts.check_only {
        println!(
            "\nCheck complete: {} file(s) would be written, {} deleted.",
            total.written, total.deleted
        );
        if !drift_changed.is_empty() {
            println!("Drift in user-owned files (would warn after a real update):");
            for p in &drift_changed {
                println!("  {p}");
            }
        }
        // Cleanup tmp clone — check mode is non-destructive end-to-end.
        let _ = fs::remove_dir_all(&tmp);
        return Ok(());
    }

    // --- Persist [applied] block. ------------------------------------------
    m.applied = Some(Applied {
        git_ref: target_ref.clone(),
        commit: upstream_commit.clone(),
        updated_at: manifest::now_iso8601(),
    });
    if opts.override_ref.is_some() {
        m.source.git_ref = target_ref.clone();
    }
    manifest::save(root, &m)?;

    // --- Cleanup tmp clone. -------------------------------------------------
    let _ = fs::remove_dir_all(&tmp);

    // --- Final summary. -----------------------------------------------------
    println!(
        "\nUpdated to {target_ref} ({}). {} file(s) written, {} deleted.",
        &upstream_commit[..upstream_commit.len().min(12)],
        total.written,
        total.deleted
    );
    if !drift_changed.is_empty() {
        println!(
            "\nNote: these user-owned files changed upstream — your local copies were\n\
             not modified. If `cargo build` fails or behavior shifts, diff against\n\
             upstream {target_ref} for the changes:"
        );
        for p in &drift_changed {
            println!("  {p}");
        }
    }
    println!("Re-running xtask will rebuild against the new sources.");
    Ok(())
}

// ============================================================================
// Sync — mirror upstream into local, deleting orphans.
// ============================================================================

fn sync_one(upstream: &Path, local: &Path, check_only: bool) -> Result<SyncStats> {
    let mut stats = SyncStats::default();

    if !upstream.exists() {
        // Upstream retired this path entirely — delete the whole local tree.
        if local.exists() {
            stats.deleted += count_files(local);
            if !check_only {
                if local.is_dir() {
                    fs::remove_dir_all(local).with_context(|| {
                        format!("Failed to remove retired path {}", local.display())
                    })?;
                } else {
                    fs::remove_file(local).with_context(|| {
                        format!("Failed to remove retired file {}", local.display())
                    })?;
                }
            }
        }
        return Ok(stats);
    }

    if upstream.is_file() {
        // Single file managed entry (e.g. `.gitignore`).
        if !check_only {
            if let Some(parent) = local.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(upstream, local).with_context(|| {
                format!(
                    "Failed to copy {} -> {}",
                    upstream.display(),
                    local.display()
                )
            })?;
        }
        stats.written += 1;
        return Ok(stats);
    }

    // Directory entry. Walk upstream → mirror; then walk local → delete orphans.
    for entry in WalkDir::new(upstream).into_iter().filter_map(|e| e.ok()) {
        let rel = entry.path().strip_prefix(upstream).unwrap();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = local.join(rel);
        if entry.file_type().is_dir() {
            if !check_only {
                fs::create_dir_all(&target)?;
            }
        } else if entry.file_type().is_file() {
            if !check_only {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(entry.path(), &target).with_context(|| {
                    format!(
                        "Failed to copy {} -> {}",
                        entry.path().display(),
                        target.display()
                    )
                })?;
            }
            stats.written += 1;
        }
    }

    if local.exists() {
        for entry in WalkDir::new(local).into_iter().filter_map(|e| e.ok()) {
            let rel = entry.path().strip_prefix(local).unwrap();
            if rel.as_os_str().is_empty() {
                continue;
            }
            // Defensive — never delete inside a nested .git or target dir
            // even if one happens to be sitting in a managed path.
            if rel.components().any(|c| {
                let s = c.as_os_str();
                s == ".git" || s == "target"
            }) {
                continue;
            }
            let upstream_counterpart = upstream.join(rel);
            if !upstream_counterpart.exists() {
                if entry.file_type().is_file() {
                    stats.deleted += 1;
                    if !check_only {
                        fs::remove_file(entry.path()).with_context(|| {
                            format!("Failed to remove orphan {}", entry.path().display())
                        })?;
                    }
                } else if entry.file_type().is_dir() {
                    if !check_only {
                        // Best-effort: only succeeds when empty after file
                        // deletions above. Empty orphan dirs that walkdir
                        // visits later get cleaned on a subsequent run.
                        let _ = fs::remove_dir(entry.path());
                    }
                }
            }
        }
    }

    Ok(stats)
}

fn count_files(p: &Path) -> u64 {
    if p.is_file() {
        return 1;
    }
    WalkDir::new(p)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count() as u64
}

// ============================================================================
// Git invocations
// ============================================================================

fn clone_at_ref(url: &str, git_ref: &str, dest: &Path) -> Result<()> {
    // Try shallow first — fast path for tags/branches.
    let shallow = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            git_ref,
            url,
            &dest.to_string_lossy(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if let Ok(s) = shallow
        && s.success()
    {
        return Ok(());
    }

    // Fallback for commit shas: full clone, then checkout.
    let full = Command::new("git")
        .args(["clone", url, &dest.to_string_lossy()])
        .status()
        .context("git clone failed")?;
    if !full.success() {
        bail!("git clone failed");
    }
    let checkout = Command::new("git")
        .args(["checkout", "--quiet", git_ref])
        .current_dir(dest)
        .status()
        .context("git checkout failed")?;
    if !checkout.success() {
        bail!("git checkout {git_ref} failed");
    }
    Ok(())
}

fn git_rev_parse(repo: &Path, rev: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output()
        .context("git rev-parse failed")?;
    if !out.status.success() {
        bail!("git rev-parse {rev} failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn diff_paths(repo: &Path, from: &str, to: &str, paths: &[&str]) -> Result<Vec<String>> {
    let range = format!("{from}..{to}");
    let mut args: Vec<&str> = vec!["diff", "--name-only", &range, "--"];
    args.extend(paths.iter().copied());
    let out = Command::new("git")
        .args(&args)
        .current_dir(repo)
        .output()
        .context("git diff failed")?;
    if !out.status.success() {
        // History may be too shallow — return empty rather than fail.
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// First-run manifest seed. Reads the current repo's origin URL and HEAD ref
/// so a freshly instantiated project remembers where it came from. Skipped
/// (with a hint) when origin is missing or the directory isn't a git repo.
pub fn write_init_manifest(root: &Path) -> Result<()> {
    let path = manifest::manifest_path(root);
    if path.exists() {
        // Don't clobber an existing manifest — re-running init shouldn't reset
        // a user's customized [source] block.
        return Ok(());
    }

    let url = match git_remote_url(root, "origin") {
        Some(u) => u,
        None => {
            println!(
                "Note: no `origin` remote detected — skipping .powertool.toml.\n\
                 Bootstrap later with: cargo xtask update --bootstrap <git-url>"
            );
            return Ok(());
        }
    };

    let git_ref = git_current_ref(root).unwrap_or_else(|| "main".to_string());

    let m = Manifest {
        schema_version: CURRENT_SCHEMA_VERSION,
        source: Source { url, git_ref },
        applied: None,
        managed_paths: ManagedPathsConfig::default(),
    };
    manifest::save(root, &m)?;
    println!("Wrote {}", path.display());
    Ok(())
}

fn git_remote_url(repo: &Path, name: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", name])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn git_current_ref(repo: &Path) -> Option<String> {
    // Prefer a tag if HEAD is exactly on one.
    if let Ok(out) = Command::new("git")
        .args(["describe", "--tags", "--exact-match"])
        .current_dir(repo)
        .stderr(Stdio::null())
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    // Otherwise pin to the commit sha.
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn list_dirty_managed_paths(root: &Path, managed: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["status", "--porcelain", "--"];
    args.extend(managed.iter().copied());
    let out = Command::new("git")
        .args(&args)
        .current_dir(root)
        .output()
        .context("git status failed")?;
    if !out.status.success() {
        // Not a git repo — nothing to guard against.
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("godot-powertool-update-{name}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sync_one_check_only_reports_without_writing() {
        let root = temp_dir("check-only");
        let upstream = root.join("upstream");
        let local = root.join("local");
        fs::create_dir_all(&upstream).unwrap();
        fs::create_dir_all(&local).unwrap();
        fs::write(upstream.join("new.txt"), "new").unwrap();

        let stats = sync_one(&upstream, &local, true).unwrap();

        assert_eq!(stats.written, 1);
        assert_eq!(stats.deleted, 0);
        assert!(!local.join("new.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sync_one_mirrors_directory_and_removes_orphans() {
        let root = temp_dir("mirror");
        let upstream = root.join("upstream");
        let local = root.join("local");
        fs::create_dir_all(upstream.join("nested")).unwrap();
        fs::create_dir_all(&local).unwrap();
        fs::write(upstream.join("nested").join("keep.txt"), "upstream").unwrap();
        fs::write(local.join("orphan.txt"), "local only").unwrap();

        let stats = sync_one(&upstream, &local, false).unwrap();

        assert_eq!(stats.written, 1);
        assert_eq!(stats.deleted, 1);
        assert_eq!(
            fs::read_to_string(local.join("nested").join("keep.txt")).unwrap(),
            "upstream"
        );
        assert!(!local.join("orphan.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sync_one_updates_single_file_without_touching_siblings() {
        let root = temp_dir("single-file");
        let upstream = root.join("upstream.txt");
        let local_dir = root.join("local");
        let local = local_dir.join("managed.txt");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(&upstream, "managed").unwrap();
        fs::write(&local, "old").unwrap();
        fs::write(local_dir.join("user.txt"), "user").unwrap();

        let stats = sync_one(&upstream, &local, false).unwrap();

        assert_eq!(stats.written, 1);
        assert_eq!(stats.deleted, 0);
        assert_eq!(fs::read_to_string(&local).unwrap(), "managed");
        assert_eq!(
            fs::read_to_string(local_dir.join("user.txt")).unwrap(),
            "user"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
