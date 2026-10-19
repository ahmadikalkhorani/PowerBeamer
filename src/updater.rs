use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use serde::Deserialize;

pub const DEFAULT_REPO: &str = "ahmadikalkhorani/PowerBeamer";

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub published_at: Option<String>,
    pub html_url: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_notes: String,
    pub download_url: String,
    pub asset_name: String,
    pub release_url: String,
}

/// Parses a semver version string like "v1.2.3" or "0.1.0" into (major, minor, patch).
pub fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let clean = v.trim().trim_start_matches('v').trim_start_matches('V');
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let major = parts[0].parse::<u64>().ok()?;
    let minor = parts[1].parse::<u64>().ok()?;
    let patch = if parts.len() >= 3 {
        parts[2].split('-').next().unwrap_or("0").parse::<u64>().unwrap_or(0)
    } else {
        0
    };
    Some((major, minor, patch))
}

/// Returns true if remote_version is strictly newer than current_version.
pub fn is_newer_version(current: &str, remote: &str) -> bool {
    match (parse_semver(current), parse_semver(remote)) {
        (Some((c_maj, c_min, c_pat)), Some((r_maj, r_min, r_pat))) => {
            if r_maj > c_maj {
                return true;
            }
            if r_maj == c_maj && r_min > c_min {
                return true;
            }
            if r_maj == c_maj && r_min == c_min && r_pat > c_pat {
                return true;
            }
            false
        }
        _ => remote != current,
    }
}

/// Detects the expected release asset file name substring for the current OS and CPU architecture.
pub fn current_platform_target() -> (&'static str, &'static str) {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        ("macos", "arm64")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        ("macos", "x86_64")
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        ("linux", "x86_64")
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        ("windows", "x86_64")
    }
    #[cfg(not(any(
        all(target_os = "macos", any(target_arch = "aarch64", target_arch = "x86_64")),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        ("unknown", "unknown")
    }
}

/// Checks the GitHub repository for the latest release.
pub fn check_for_update(repo: &str) -> Result<Option<UpdateInfo>, String> {
    let current_version = env!("CARGO_PKG_VERSION");
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);

    let resp = ureq::get(&url)
        .set("User-Agent", "Presenter-AutoUpdater")
        .set("Accept", "application/vnd.github.v3+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("Failed to connect to GitHub Releases: {}", e))?;

    let release: GithubRelease = resp
        .into_json()
        .map_err(|e| format!("Failed to parse release information: {}", e))?;

    if !is_newer_version(current_version, &release.tag_name) {
        return Ok(None);
    }

    let (os_str, arch_str) = current_platform_target();
    let matching_asset = release.assets.iter().find(|asset| {
        let name_lower = asset.name.to_lowercase();
        name_lower.contains(os_str) && (name_lower.contains(arch_str) || (arch_str == "arm64" && name_lower.contains("aarch64")))
    });

    let (download_url, asset_name) = if let Some(asset) = matching_asset {
        (asset.browser_download_url.clone(), asset.name.clone())
    } else if let Some(first) = release.assets.first() {
        (first.browser_download_url.clone(), first.name.clone())
    } else {
        return Err(format!("Release {} found, but no pre-built binaries are attached.", release.tag_name));
    };

    Ok(Some(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: release.tag_name,
        release_notes: release.body.unwrap_or_else(|| "No release notes provided.".to_string()),
        download_url,
        asset_name,
        release_url: release.html_url,
    }))
}

/// Downloads the release archive and replaces the current executable.
pub fn download_and_install_update<F: Fn(f32) + Send + 'static>(
    download_url: &str,
    progress_callback: F,
) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("Could not get current executable path: {}", e))?;
    let temp_dir = std::env::temp_dir().join(format!("presenter_update_{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {}", e))?;

    let archive_path = temp_dir.join("update_archive.tar.gz");

    // 1. Download asset with stream progress
    let resp = ureq::get(download_url)
        .set("User-Agent", "Presenter-AutoUpdater")
        .call()
        .map_err(|e| format!("Failed to download update: {}", e))?;

    let total_size = resp.header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut reader = resp.into_reader();
    let mut file = File::create(&archive_path).map_err(|e| format!("Failed to write update file: {}", e))?;

    let mut buffer = [0u8; 16384];
    let mut downloaded = 0u64;

    loop {
        let bytes_read = reader.read(&mut buffer).map_err(|e| format!("Download stream error: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        file.write_all(&buffer[..bytes_read]).map_err(|e| format!("Failed to write archive chunk: {}", e))?;
        downloaded += bytes_read as u64;
        if total_size > 0 {
            let progress = (downloaded as f32 / total_size as f32).clamp(0.0, 1.0);
            progress_callback(progress);
        }
    }
    progress_callback(1.0);

    // 2. Extract tar.gz archive
    let tar_gz = File::open(&archive_path).map_err(|e| format!("Failed to read downloaded archive: {}", e))?;
    let tar = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(tar);

    let extract_dir = temp_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir).map_err(|e| format!("Failed to create extract folder: {}", e))?;
    archive.unpack(&extract_dir).map_err(|e| format!("Failed to unpack archive: {}", e))?;

    // 3. Find binary in extracted directory
    let new_binary = find_binary_in_dir(&extract_dir, "presenter")
        .ok_or_else(|| "Could not locate 'presenter' binary in downloaded release archive.".to_string())?;

    // 4. Safely replace the running binary
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&new_binary)
            .map_err(|e| format!("Failed to read new binary metadata: {}", e))?
            .permissions();
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(&new_binary, perms);

        // Atomic swap using backup file
        let backup_exe = current_exe.with_extension("old");
        if backup_exe.exists() {
            let _ = std::fs::remove_file(&backup_exe);
        }
        std::fs::rename(&current_exe, &backup_exe)
            .map_err(|e| format!("Failed to move old executable: {}", e))?;

        if let Err(e) = std::fs::copy(&new_binary, &current_exe) {
            // Restore backup on failure
            let _ = std::fs::rename(&backup_exe, &current_exe);
            return Err(format!("Failed to copy new executable into place: {}", e));
        }

        let _ = std::fs::remove_file(backup_exe);
    }

    #[cfg(windows)]
    {
        let backup_exe = current_exe.with_extension("old");
        if backup_exe.exists() {
            let _ = std::fs::remove_file(&backup_exe);
        }
        std::fs::rename(&current_exe, &backup_exe)
            .map_err(|e| format!("Failed to move old executable: {}", e))?;
        if let Err(e) = std::fs::copy(&new_binary, &current_exe) {
            let _ = std::fs::rename(&backup_exe, &current_exe);
            return Err(format!("Failed to copy new executable: {}", e));
        }
    }

    // Clean up temporary update directory
    let _ = std::fs::remove_dir_all(temp_dir);

    Ok(())
}

fn find_binary_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                    if file_name == name || file_name == format!("{}.exe", name) {
                        return Some(path);
                    }
                }
            } else if path.is_dir() {
                if let Some(found) = find_binary_in_dir(&path, name) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Restarts the application using the updated executable and passes original CLI arguments.
pub fn restart_application() {
    if let Ok(current_exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut cmd = std::process::Command::new(current_exe);
        cmd.args(args);
        let _ = cmd.spawn();
        std::process::exit(0);
    }
}
