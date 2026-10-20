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

    let asset = platform_asset(&release)
        .ok_or_else(|| format!("Release {} found, but no pre-built binaries are attached.", release.tag_name))?;
    let (download_url, asset_name) = (asset.browser_download_url.clone(), asset.name.clone());

    Ok(Some(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: release.tag_name,
        release_notes: release.body.unwrap_or_else(|| "No release notes provided.".to_string()),
        download_url,
        asset_name,
        release_url: release.html_url,
    }))
}

/// The release asset built for this OS/CPU, falling back to the first asset.
fn platform_asset(release: &GithubRelease) -> Option<&ReleaseAsset> {
    let (os_str, arch_str) = current_platform_target();
    release
        .assets
        .iter()
        .find(|asset| {
            let name_lower = asset.name.to_lowercase();
            name_lower.contains(os_str)
                && (name_lower.contains(arch_str) || (arch_str == "arm64" && name_lower.contains("aarch64")))
        })
        .or_else(|| release.assets.first())
}

/// Repairs an app bundle left half-updated by updaters older than 0.1.2, which replaced only
/// the executable (leaving the old Info.plist, icons and a broken signature). If the bundle's
/// Info.plist version doesn't match this binary, re-install this version's full release
/// bundle; it takes effect on the next launch. Returns `Ok(true)` when a repair was installed.
pub fn repair_stale_bundle(repo: &str) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let version = env!("CARGO_PKG_VERSION");
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let Some(app) = enclosing_app_bundle(&exe) else { return Ok(false) };
        let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap_or_default();
        if bundle_short_version(&plist).as_deref() == Some(version) {
            return Ok(false);
        }

        let url = format!("https://api.github.com/repos/{}/releases/tags/v{}", repo, version);
        let release: GithubRelease = ureq::get(&url)
            .set("User-Agent", "Presenter-AutoUpdater")
            .set("Accept", "application/vnd.github.v3+json")
            .timeout(std::time::Duration::from_secs(10))
            .call()
            .map_err(|e| format!("Couldn't fetch release v{}: {}", version, e))?
            .into_json()
            .map_err(|e| format!("Couldn't parse release v{}: {}", version, e))?;
        let asset = platform_asset(&release).ok_or("release has no assets")?;
        download_and_install_update(&asset.browser_download_url, |_| {})?;
        Ok(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = repo;
        Ok(false)
    }
}

/// `CFBundleShortVersionString` from an Info.plist's XML.
#[cfg(target_os = "macos")]
fn bundle_short_version(plist: &str) -> Option<String> {
    let after_key = plist.split("<key>CFBundleShortVersionString</key>").nth(1)?;
    let value = after_key.split("<string>").nth(1)?.split("</string>").next()?;
    Some(value.trim().to_string())
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

    // 3a. macOS app bundle: replace the whole PowerBeamer.app (binary, Info.plist, icons,
    // signature) so the installed app is exactly the released one.
    #[cfg(target_os = "macos")]
    if let (Some(installed_app), Some(new_app)) =
        (enclosing_app_bundle(&current_exe), find_app_bundle_in_dir(&extract_dir))
    {
        replace_app_bundle(&new_app, &installed_app)?;
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Ok(());
    }

    // 3b. Bare executable (Linux, Windows, or running outside a bundle). Current releases name
    // it `powerbeamer`; `presenter` is the pre-0.1 name.
    let new_binary = find_binary_in_dir(&extract_dir, "powerbeamer")
        .or_else(|| find_binary_in_dir(&extract_dir, "presenter"))
        .ok_or_else(|| "Could not locate the PowerBeamer executable in the downloaded release archive.".to_string())?;

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

/// `/Applications/PowerBeamer.app` for an executable at `.../PowerBeamer.app/Contents/MacOS/x`.
#[cfg(target_os = "macos")]
fn enclosing_app_bundle(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    let contents = macos_dir.parent()?;
    let app = contents.parent()?;
    let is_bundle = macos_dir.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension()? == "app";
    is_bundle.then(|| app.to_path_buf())
}

#[cfg(target_os = "macos")]
fn find_app_bundle_in_dir(dir: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() || path.is_symlink() {
            continue;
        }
        if path.extension().is_some_and(|e| e == "app") && path.join("Contents/MacOS").is_dir() {
            return Some(path);
        }
        if let Some(found) = find_app_bundle_in_dir(&path) {
            return Some(found);
        }
    }
    None
}

/// Swaps `installed` for `new_app`. The new bundle is first copied next to the installed one
/// (same volume, so the final step is an atomic rename); the old bundle is restored if any
/// step fails. Replacing the files of a running app is safe on macOS: the process keeps its
/// already-open executable until it restarts.
#[cfg(target_os = "macos")]
fn replace_app_bundle(new_app: &Path, installed: &Path) -> Result<(), String> {
    let staged = installed.with_extension("app.updating");
    let backup = installed.with_extension("app.old");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&backup);

    copy_dir_preserving(new_app, &staged).map_err(|e| {
        let _ = std::fs::remove_dir_all(&staged);
        format!(
            "Couldn't write the update next to {} ({}). Download the new version from GitHub and replace the app manually.",
            installed.display(),
            e
        )
    })?;
    // The archive is fetched by us (not a browser) so it shouldn't be quarantined, but clear
    // the flag anyway so Gatekeeper never blocks the relaunch.
    let _ = std::process::Command::new("xattr").args(["-cr"]).arg(&staged).status();

    std::fs::rename(installed, &backup).map_err(|e| {
        let _ = std::fs::remove_dir_all(&staged);
        format!("Failed to move the old app aside: {}", e)
    })?;
    if let Err(e) = std::fs::rename(&staged, installed) {
        let _ = std::fs::rename(&backup, installed);
        let _ = std::fs::remove_dir_all(&staged);
        return Err(format!("Failed to install the new app: {}", e));
    }
    let _ = std::fs::remove_dir_all(&backup);
    Ok(())
}

/// Recursive copy that keeps symlinks as symlinks and file permissions intact (both matter
/// inside an app bundle and for its code signature).
#[cfg(target_os = "macos")]
fn copy_dir_preserving(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
        } else if kind.is_dir() {
            copy_dir_preserving(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
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
    // Relaunch a bundled app through LaunchServices so macOS picks up the new bundle
    // (Info.plist, icon, signature) rather than just re-running the executable.
    #[cfg(target_os = "macos")]
    if let Some(app) = std::env::current_exe().ok().as_deref().and_then(enclosing_app_bundle) {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut cmd = std::process::Command::new("open");
        cmd.arg("-n").arg(&app);
        if !args.is_empty() {
            cmd.arg("--args").args(args);
        }
        if cmd.spawn().is_ok() {
            std::process::exit(0);
        }
    }
    if let Ok(current_exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut cmd = std::process::Command::new(current_exe);
        cmd.args(args);
        let _ = cmd.spawn();
        std::process::exit(0);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn replaces_whole_app_bundle_and_keeps_symlinks() {
        let root = std::env::temp_dir().join(format!("pb_update_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let installed = root.join("Applications/PowerBeamer.app");
        write(&installed.join("Contents/MacOS/powerbeamer"), "old binary");
        write(&installed.join("Contents/Info.plist"), "0.1.0");
        write(&installed.join("Contents/MacOS/stale_leftover"), "x");

        let new_app = root.join("extracted/PowerBeamer.app");
        write(&new_app.join("Contents/MacOS/powerbeamer"), "new binary");
        write(&new_app.join("Contents/Info.plist"), "0.1.1");
        std::os::unix::fs::symlink("powerbeamer", new_app.join("Contents/MacOS/presenter")).unwrap();

        assert_eq!(enclosing_app_bundle(&installed.join("Contents/MacOS/powerbeamer")), Some(installed.clone()));
        assert_eq!(find_app_bundle_in_dir(&root.join("extracted")), Some(new_app.clone()));

        replace_app_bundle(&new_app, &installed).unwrap();
        let read = |p: &str| std::fs::read_to_string(installed.join(p)).unwrap();
        assert_eq!(read("Contents/MacOS/powerbeamer"), "new binary");
        assert_eq!(read("Contents/Info.plist"), "0.1.1");
        assert!(!installed.join("Contents/MacOS/stale_leftover").exists(), "old files are gone");
        assert!(installed.join("Contents/MacOS/presenter").is_symlink(), "symlinks preserved");
        assert!(!installed.with_extension("app.old").exists() && !installed.with_extension("app.updating").exists());
        assert_eq!(
            bundle_short_version("<dict>\n  <key>CFBundleShortVersionString</key>\n  <string>0.1.1</string>\n</dict>").as_deref(),
            Some("0.1.1")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

