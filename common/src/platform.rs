use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::Result;

/// Find the Godot binary, checking multiple sources in priority order:
/// 1. `GODOT_PATH` environment variable
/// 2. Project-local `tools/godot/godot` (if project_root provided)
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

    // 2. Project-local tools directory
    if let Some(root) = project_root {
        let local = root.join("tools").join("godot").join("godot");
        if local.exists() {
            return Ok(local);
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

    #[cfg(target_os = "macos")]
    {
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

    #[cfg(target_os = "windows")]
    {
        let userprofile = env::var("USERPROFILE").unwrap_or_default();
        vec![
            r"C:\Program Files\Godot\Godot.exe".into(),
            r"C:\Program Files (x86)\Godot\Godot.exe".into(),
            r"C:\Program Files\Godot_4\Godot.exe".into(),
            r"C:\Program Files (x86)\Godot_4\Godot.exe".into(),
            format!(r"{userprofile}\Godot\Godot.exe"),
        ]
    }

    #[cfg(target_os = "linux")]
    {
        vec![
            "/usr/bin/godot".into(),
            "/usr/local/bin/godot".into(),
            "/snap/bin/godot".into(),
            format!("{home}/.local/bin/godot"),
        ]
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        vec!["/usr/bin/godot".into(), "/usr/local/bin/godot".into()]
    }
}

/// Resolve the Godot `user://` data directory for a given project name.
/// This is where Godot stores per-project user data (saves, screenshots, etc.).
pub fn godot_user_data_dir(project_name: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = env::var("APPDATA").unwrap_or_default();
        PathBuf::from(appdata)
            .join("Godot")
            .join("app_userdata")
            .join(project_name)
    }

    #[cfg(target_os = "macos")]
    {
        let home = env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Godot")
            .join("app_userdata")
            .join(project_name)
    }

    #[cfg(target_os = "linux")]
    {
        let home = env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("godot")
            .join("app_userdata")
            .join(project_name)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let home = env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("godot")
            .join("app_userdata")
            .join(project_name)
    }
}

/// Get the platform-specific extension library filename.
pub fn extension_lib_name() -> &'static str {
    #[cfg(target_os = "linux")]
    return "libextension.so";
    #[cfg(target_os = "windows")]
    return "extension.dll";
    #[cfg(target_os = "macos")]
    return "libextension.dylib";
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    return "libextension.so";
}

/// Get the platform subdirectory name for extension binaries.
pub fn extension_platform_dir() -> &'static str {
    #[cfg(target_os = "linux")]
    return "linux";
    #[cfg(target_os = "windows")]
    return "windows";
    #[cfg(target_os = "macos")]
    return "macos";
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    return "linux";
}

/// Returns the Godot download URL and expected binary name within the archive.
pub fn godot_download_info(version: &str) -> (String, String) {
    #[cfg(target_os = "linux")]
    let (archive_name, binary_name) = (
        format!("Godot_v{version}-stable_linux.x86_64.zip"),
        format!("Godot_v{version}-stable_linux.x86_64"),
    );
    #[cfg(target_os = "macos")]
    let (archive_name, binary_name) = (
        format!("Godot_v{version}-stable_macos.universal.zip"),
        "Godot.app/Contents/MacOS/Godot".to_string(),
    );
    #[cfg(target_os = "windows")]
    let (archive_name, binary_name) = (
        format!("Godot_v{version}-stable_win64.exe.zip"),
        format!("Godot_v{version}-stable_win64.exe"),
    );
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let (archive_name, binary_name) = (
        format!("Godot_v{version}-stable_linux.x86_64.zip"),
        format!("Godot_v{version}-stable_linux.x86_64"),
    );

    let url = format!(
        "https://github.com/godotengine/godot/releases/download/{version}-stable/{archive_name}"
    );
    (url, binary_name)
}

/// Open a path in the platform's default application (browser, file manager, etc.).
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
