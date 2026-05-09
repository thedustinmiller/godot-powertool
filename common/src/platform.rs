use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum HostPlatform {
    Linux,
    Macos,
    Windows,
    Other,
}

impl HostPlatform {
    fn current() -> Self {
        #[cfg(target_os = "linux")]
        return Self::Linux;
        #[cfg(target_os = "macos")]
        return Self::Macos;
        #[cfg(target_os = "windows")]
        return Self::Windows;
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        return Self::Other;
    }
}

/// Per-host subdirectory name used under `tools/godot/` so that side-by-side
/// setups (e.g. WSL Linux + native Windows on the same checkout) keep separate
/// binaries instead of clobbering each other.
pub fn tools_subdir_name() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

/// Filename for the normalized Godot binary inside the per-host subdir.
pub fn local_godot_binary_name() -> &'static str {
    if cfg!(target_os = "windows") { "godot.exe" } else { "godot" }
}

/// Path to the project-local Godot binary for the current host:
/// `<root>/tools/godot/<arch>-<os>/godot[.exe]`.
pub fn local_godot_binary_path(project_root: &Path) -> PathBuf {
    project_root
        .join("tools")
        .join("godot")
        .join(tools_subdir_name())
        .join(local_godot_binary_name())
}

/// Find the Godot binary, checking multiple sources in priority order:
/// 1. `GODOT_PATH` environment variable
/// 2. Project-local `tools/godot/<arch>-<os>/godot[.exe]` (if project_root provided)
/// 3. `godot` on system PATH
/// 4. Platform-specific well-known locations
pub fn find_godot_binary(project_root: Option<&Path>) -> Result<PathBuf> {
    // 1. Environment variable override
    if let Ok(path) = env::var("GODOT_PATH") {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Ok(p);
        }
    }

    // 2. Project-local tools directory (per-host subdir)
    if let Some(root) = project_root {
        let local = local_godot_binary_path(root);
        if local.exists() {
            return Ok(local);
        }
        // Backwards-compat: pre-multi-platform layout. Re-running
        // `cargo xtask setup` migrates to the per-host subdir.
        let legacy = root.join("tools").join("godot").join("godot");
        if legacy.exists() {
            return Ok(legacy);
        }
    }

    // 3. System PATH
    if let Ok(path) = which::which("godot") {
        return Ok(path);
    }

    // 4. Platform-specific well-known paths
    for path in platform_godot_paths() {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    anyhow::bail!(
        "Godot not found. Options:\n  \
         - Set GODOT_PATH environment variable\n  \
         - Run 'cargo xtask setup' to download it\n  \
         - Install Godot system-wide"
    )
}

/// Returns platform-specific Godot binary paths to check.
fn platform_godot_paths() -> Vec<String> {
    let home = env::var("HOME").unwrap_or_default();
    let userprofile = env::var("USERPROFILE").unwrap_or_default();
    platform_godot_paths_for(HostPlatform::current(), &home, &userprofile)
}

fn platform_godot_paths_for(platform: HostPlatform, home: &str, userprofile: &str) -> Vec<String> {
    match platform {
        HostPlatform::Macos => {
            vec![
                "/Applications/Godot.app/Contents/MacOS/Godot".into(),
                "/Applications/Godot_4.app/Contents/MacOS/Godot".into(),
                format!("{home}/Applications/Godot.app/Contents/MacOS/Godot"),
                format!("{home}/Applications/Godot_4.app/Contents/MacOS/Godot"),
                format!(
                    "{home}/Library/Application Support/Steam/steamapps/common/Godot Engine/Godot.app/Contents/MacOS/Godot"
                ),
            ]
        }
        HostPlatform::Windows => vec![
            r"C:\Program Files\Godot\Godot.exe".into(),
            r"C:\Program Files (x86)\Godot\Godot.exe".into(),
            r"C:\Program Files\Godot_4\Godot.exe".into(),
            r"C:\Program Files (x86)\Godot_4\Godot.exe".into(),
            format!(r"{userprofile}\Godot\Godot.exe"),
        ],
        HostPlatform::Linux => vec![
            "/usr/bin/godot".into(),
            "/usr/local/bin/godot".into(),
            "/snap/bin/godot".into(),
            format!("{home}/.local/bin/godot"),
        ],
        HostPlatform::Other => vec!["/usr/bin/godot".into(), "/usr/local/bin/godot".into()],
    }
}

/// Resolve the Godot `user://` data directory for a given project name.
/// This is where Godot stores per-project user data (saves, screenshots, etc.).
pub fn godot_user_data_dir(project_name: &str) -> PathBuf {
    godot_user_data_dir_for(
        HostPlatform::current(),
        &env::var("HOME").unwrap_or_default(),
        &env::var("APPDATA").unwrap_or_default(),
        project_name,
    )
}

fn godot_user_data_dir_for(
    platform: HostPlatform,
    home: &str,
    appdata: &str,
    project_name: &str,
) -> PathBuf {
    match platform {
        HostPlatform::Windows => PathBuf::from(appdata)
            .join("Godot")
            .join("app_userdata")
            .join(project_name),
        HostPlatform::Macos => PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Godot")
            .join("app_userdata")
            .join(project_name),
        HostPlatform::Linux | HostPlatform::Other => PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("godot")
            .join("app_userdata")
            .join(project_name),
    }
}

/// Get the platform-specific extension library filename for a given basename.
///
/// `basename` is the GDExtension's library basename — e.g. `sample_extension`
/// produces `libsample_extension.so` on Linux, `sample_extension.dll` on
/// Windows, `libsample_extension.dylib` on macOS. Configured via
/// `[extension].lib_basename` in `template.toml`.
pub fn extension_lib_name(basename: &str) -> String {
    extension_lib_name_for(HostPlatform::current(), basename)
}

fn extension_lib_name_for(platform: HostPlatform, basename: &str) -> String {
    match platform {
        HostPlatform::Windows => format!("{basename}.dll"),
        HostPlatform::Macos => format!("lib{basename}.dylib"),
        HostPlatform::Linux | HostPlatform::Other => format!("lib{basename}.so"),
    }
}

/// Get the platform subdirectory name for extension binaries.
pub fn extension_platform_dir() -> &'static str {
    extension_platform_dir_for(HostPlatform::current())
}

fn extension_platform_dir_for(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Windows => "windows",
        HostPlatform::Macos => "macos",
        HostPlatform::Linux | HostPlatform::Other => "linux",
    }
}

/// Returns the Godot download URL and expected binary name within the archive.
pub fn godot_download_info(version: &str) -> (String, String) {
    godot_download_info_for(HostPlatform::current(), version)
}

fn godot_download_info_for(platform: HostPlatform, version: &str) -> (String, String) {
    let (archive_name, binary_name) = match platform {
        HostPlatform::Linux | HostPlatform::Other => (
            format!("Godot_v{version}-stable_linux.x86_64.zip"),
            format!("Godot_v{version}-stable_linux.x86_64"),
        ),
        HostPlatform::Macos => (
            format!("Godot_v{version}-stable_macos.universal.zip"),
            "Godot.app/Contents/MacOS/Godot".to_string(),
        ),
        HostPlatform::Windows => (
            format!("Godot_v{version}-stable_win64.exe.zip"),
            format!("Godot_v{version}-stable_win64.exe"),
        ),
    };
    let url = format!(
        "https://github.com/godotengine/godot/releases/download/{version}-stable/{archive_name}"
    );
    (url, binary_name)
}

/// Open a path in the platform's default application (browser, file manager,
/// etc.).
pub fn open_path(path: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_platform_values_are_stable() {
        assert_eq!(
            extension_lib_name_for(HostPlatform::Linux, "sample_extension"),
            "libsample_extension.so"
        );
        assert_eq!(extension_platform_dir_for(HostPlatform::Linux), "linux");
        assert_eq!(
            godot_user_data_dir_for(HostPlatform::Linux, "/home/alice", "", "Project"),
            PathBuf::from("/home/alice/.local/share/godot/app_userdata/Project")
        );

        let (url, binary) = godot_download_info_for(HostPlatform::Linux, "4.6.2");
        assert!(url.ends_with("/4.6.2-stable/Godot_v4.6.2-stable_linux.x86_64.zip"));
        assert_eq!(binary, "Godot_v4.6.2-stable_linux.x86_64");

        let paths = platform_godot_paths_for(HostPlatform::Linux, "/home/alice", "");
        assert!(paths.contains(&"/usr/bin/godot".to_string()));
        assert!(paths.contains(&"/home/alice/.local/bin/godot".to_string()));
    }

    #[test]
    fn macos_platform_values_are_stable() {
        assert_eq!(
            extension_lib_name_for(HostPlatform::Macos, "sample_extension"),
            "libsample_extension.dylib"
        );
        assert_eq!(extension_platform_dir_for(HostPlatform::Macos), "macos");
        assert_eq!(
            godot_user_data_dir_for(HostPlatform::Macos, "/Users/alice", "", "Project"),
            PathBuf::from("/Users/alice/Library/Application Support/Godot/app_userdata/Project")
        );

        let (url, binary) = godot_download_info_for(HostPlatform::Macos, "4.6.2");
        assert!(url.ends_with("/4.6.2-stable/Godot_v4.6.2-stable_macos.universal.zip"));
        assert_eq!(binary, "Godot.app/Contents/MacOS/Godot");

        let paths = platform_godot_paths_for(HostPlatform::Macos, "/Users/alice", "");
        assert!(paths.contains(&"/Applications/Godot.app/Contents/MacOS/Godot".to_string()));
        assert!(
            paths.contains(&"/Users/alice/Applications/Godot.app/Contents/MacOS/Godot".to_string())
        );
    }

    #[test]
    fn windows_platform_values_are_stable() {
        assert_eq!(
            extension_lib_name_for(HostPlatform::Windows, "sample_extension"),
            "sample_extension.dll"
        );
        assert_eq!(extension_platform_dir_for(HostPlatform::Windows), "windows");
        assert_eq!(
            godot_user_data_dir_for(
                HostPlatform::Windows,
                "",
                r"C:\Users\Alice\AppData\Roaming",
                "Project"
            ),
            PathBuf::from(r"C:\Users\Alice\AppData\Roaming")
                .join("Godot")
                .join("app_userdata")
                .join("Project")
        );

        let (url, binary) = godot_download_info_for(HostPlatform::Windows, "4.6.2");
        assert!(url.ends_with("/4.6.2-stable/Godot_v4.6.2-stable_win64.exe.zip"));
        assert_eq!(binary, "Godot_v4.6.2-stable_win64.exe");

        let paths = platform_godot_paths_for(HostPlatform::Windows, "", r"C:\Users\Alice");
        assert!(paths.contains(&r"C:\Program Files\Godot\Godot.exe".to_string()));
        assert!(paths.contains(&r"C:\Users\Alice\Godot\Godot.exe".to_string()));
    }
}
