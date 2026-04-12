use std::env;
use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

const INSTALLER_URL: &str =
    "https://github.com/pupkit-labs/pupkit-cli/releases/latest/download/pupkit-installer.sh";
const RELEASE_API_URL: &str = "https://api.github.com/repos/pupkit-labs/pupkit-cli/releases/latest";
const GITHUB_ACCEPT_HEADER: &str = "accept: application/vnd.github+json";
const GITHUB_USER_AGENT_HEADER: &str = "user-agent: pupkit";

pub fn execute() -> Result<(), String> {
    let current_exe = env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let binary_name = current_exe
        .file_name()
        .ok_or_else(|| "failed to determine current executable name".to_string())?;
    let install_bin = shell_installer_bin_path(binary_name)?;
    let plan = plan_update(&current_exe, &install_bin)?;
    let current_version = current_version();
    let latest_release = fetch_latest_release_version()?;

    match compare_versions(&current_version, &latest_release.version) {
        VersionOrdering::Equal => {
            println!(
                "pupkit {} is already up to date.",
                latest_release.display_tag()
            );
            return Ok(());
        }
        VersionOrdering::CurrentIsNewer => {
            println!(
                "pupkit {} is newer than the latest published release {}; skipping update.",
                current_version,
                latest_release.display_tag()
            );
            return Ok(());
        }
        VersionOrdering::CurrentIsOlder | VersionOrdering::Unknown => {}
    }

    run_shell_installer()?;

    if let Some(sync_target) = plan.sync_target {
        sync_installed_binary(&plan.install_bin, &sync_target)?;
        println!(
            "pupkit updated from {} to {} and synced to {}",
            current_version,
            latest_release.display_tag(),
            sync_target.display()
        );
    } else {
        println!(
            "pupkit updated from {} to {}.",
            current_version,
            latest_release.display_tag()
        );
    }

    // On macOS, also update PupkitShell from the release archive
    #[cfg(target_os = "macos")]
    {
        let install_dir = plan.install_bin.parent().unwrap_or(&plan.install_bin);
        if let Err(error) = update_pupkit_shell(install_dir) {
            eprintln!("warning: PupkitShell update skipped: {error}");
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UpdatePlan {
    install_bin: PathBuf,
    sync_target: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LatestRelease {
    tag_name: String,
    version: String,
}

impl LatestRelease {
    fn display_tag(&self) -> &str {
        &self.tag_name
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VersionOrdering {
    CurrentIsOlder,
    Equal,
    CurrentIsNewer,
    Unknown,
}

fn shell_installer_bin_path(binary_name: &OsStr) -> Result<PathBuf, String> {
    Ok(cargo_home_bin_dir()?.join(binary_name))
}

fn cargo_home_bin_dir() -> Result<PathBuf, String> {
    cargo_home_bin_dir_from(
        env::var_os("CARGO_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
        env::current_dir().ok(),
    )
}

fn cargo_home_bin_dir_from(
    cargo_home: Option<PathBuf>,
    home: Option<PathBuf>,
    current_dir: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(cargo_home) = cargo_home {
        let base = if cargo_home.is_absolute() {
            cargo_home
        } else {
            current_dir
                .ok_or_else(|| "failed to resolve current directory".to_string())?
                .join(cargo_home)
        };
        return Ok(base.join("bin"));
    }

    let home = home.ok_or_else(|| "HOME is not set; cannot determine install path".to_string())?;
    Ok(home.join(".cargo").join("bin"))
}

fn plan_update(current_exe: &Path, install_bin: &Path) -> Result<UpdatePlan, String> {
    if is_homebrew_install(current_exe) {
        return Err(format!(
            "pupkit appears to be installed via Homebrew at {}; run `brew upgrade pupkit` instead",
            current_exe.display()
        ));
    }

    if is_source_build_path(current_exe) {
        return Err(format!(
            "pupkit appears to be running from a local cargo build at {}; rebuild from source or reinstall via the shell installer instead",
            current_exe.display()
        ));
    }

    let sync_target =
        if current_exe == install_bin || paths_resolve_to_same_file(current_exe, install_bin) {
            None
        } else {
            Some(current_exe.to_path_buf())
        };

    Ok(UpdatePlan {
        install_bin: install_bin.to_path_buf(),
        sync_target,
    })
}

fn is_homebrew_install(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.contains("/Cellar/") || text.contains("/.linuxbrew/") || text.contains("/Homebrew/")
}

fn is_source_build_path(path: &Path) -> bool {
    let mut saw_profile_dir = false;

    for ancestor in path.ancestors() {
        let Some(name) = ancestor.file_name().and_then(|value| value.to_str()) else {
            continue;
        };

        if name == "debug" || name == "release" {
            saw_profile_dir = true;
            continue;
        }

        if saw_profile_dir && name == "target" {
            return true;
        }
    }

    false
}

fn paths_resolve_to_same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn fetch_latest_release_version() -> Result<LatestRelease, String> {
    let body = fetch_release_metadata()?;
    parse_latest_release(&body)
}

fn fetch_release_metadata() -> Result<String, String> {
    if command_exists("curl") {
        return run_download_command(
            "curl",
            &[
                "-fsSL",
                "-H",
                GITHUB_ACCEPT_HEADER,
                "-H",
                GITHUB_USER_AGENT_HEADER,
                RELEASE_API_URL,
            ],
        );
    }

    if command_exists("wget") {
        return run_download_command(
            "wget",
            &[
                "--header",
                GITHUB_ACCEPT_HEADER,
                "--header",
                GITHUB_USER_AGENT_HEADER,
                "-q",
                "-O",
                "-",
                RELEASE_API_URL,
            ],
        );
    }

    Err("update requires either `curl` or `wget` to be available on PATH".to_string())
}

fn run_download_command(command_name: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command_name)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("failed to run {command_name}: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "{command_name} exited with status {}",
            output.status
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|error| format!("failed to parse {command_name} output as UTF-8: {error}"))
}

fn parse_latest_release(body: &str) -> Result<LatestRelease, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|error| format!("failed to parse latest release metadata: {error}"))?;
    let tag_name = value
        .get("tag_name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "latest release metadata did not include a valid tag_name".to_string())?;
    let version = normalize_version(tag_name)
        .ok_or_else(|| format!("unsupported release tag format: {tag_name}"))?;

    Ok(LatestRelease {
        tag_name: tag_name.to_string(),
        version,
    })
}

fn normalize_version(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);

    if trimmed.is_empty() || parse_semver_core(trimmed).is_none() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn compare_versions(current: &str, latest: &str) -> VersionOrdering {
    let Some(current_parts) = parse_semver_core(current) else {
        return VersionOrdering::Unknown;
    };
    let Some(latest_parts) = parse_semver_core(latest) else {
        return VersionOrdering::Unknown;
    };

    match current_parts.cmp(&latest_parts) {
        std::cmp::Ordering::Less => VersionOrdering::CurrentIsOlder,
        std::cmp::Ordering::Equal => VersionOrdering::Equal,
        std::cmp::Ordering::Greater => VersionOrdering::CurrentIsNewer,
    }
}

fn parse_semver_core(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.split(['-', '+']).next()?.trim();
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;

    if parts.next().is_some() {
        None
    } else {
        Some((major, minor, patch))
    }
}

fn run_shell_installer() -> Result<(), String> {
    if command_exists("curl") {
        return run_installer_pipeline(
            "curl",
            &["--proto", "=https", "--tlsv1.2", "-LsSf", INSTALLER_URL],
        );
    }

    if command_exists("wget") {
        return run_installer_pipeline("wget", &["-q", "-O", "-", INSTALLER_URL]);
    }

    Err("update requires either `curl` or `wget` to be available on PATH".to_string())
}

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn run_installer_pipeline(download_command: &str, download_args: &[&str]) -> Result<(), String> {
    let mut downloader = Command::new(download_command)
        .args(download_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("failed to start {download_command}: {error}"))?;

    let stdout = downloader
        .stdout
        .take()
        .ok_or_else(|| format!("failed to capture stdout from {download_command}"))?;

    let installer_status = Command::new("sh")
        .arg("-s")
        .stdin(Stdio::from(stdout))
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("failed to run shell installer: {error}"))?;

    let downloader_status = downloader
        .wait()
        .map_err(|error| format!("failed while waiting for {download_command}: {error}"))?;

    if !downloader_status.success() {
        return Err(format!(
            "{download_command} exited with status {downloader_status}"
        ));
    }

    if !installer_status.success() {
        return Err(format!(
            "shell installer exited with status {installer_status}"
        ));
    }

    Ok(())
}

fn sync_installed_binary(source: &Path, target: &Path) -> Result<(), String> {
    if source == target || paths_resolve_to_same_file(source, target) {
        return Ok(());
    }

    if !source.is_file() {
        return Err(format!(
            "installer did not produce an updated binary at {}",
            source.display()
        ));
    }

    let parent = target.parent().ok_or_else(|| {
        format!(
            "failed to determine the install directory for {}",
            target.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to prepare {}: {error}", parent.display()))?;

    let temp_name = format!(
        ".{}.tmp-{}",
        target
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("pupkit"),
        std::process::id()
    );
    let temp_path = parent.join(temp_name);

    fs::copy(source, &temp_path).map_err(|error| {
        format!(
            "failed to copy updated binary from {} to {}: {error}",
            source.display(),
            temp_path.display()
        )
    })?;

    if let Ok(metadata) = fs::metadata(source) {
        let _ = fs::set_permissions(&temp_path, metadata.permissions());
    }

    if let Err(error) = fs::rename(&temp_path, target) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "failed to replace {} with the updated binary: {error}",
            target.display()
        ));
    }

    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(source) {
        let mode = metadata.permissions().mode() & 0o777;
        let _ = fs::set_permissions(target, fs::Permissions::from_mode(mode));
    }

    Ok(())
}

/// Download PupkitShell from the latest release archive and install alongside pupkit.
#[cfg(target_os = "macos")]
fn update_pupkit_shell(install_dir: &Path) -> Result<(), String> {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    };

    let archive_url = format!(
        "https://github.com/pupkit-labs/pupkit-cli/releases/latest/download/pupkit-{arch}.tar.xz"
    );

    let shell_target = install_dir.join("PupkitShell");
    let bundle_target = install_dir.join("PupkitShell_PupkitShell.bundle");

    let script = format!(
        r#"
        set -e
        tmpdir=$(mktemp -d)
        trap 'rm -rf "$tmpdir"' EXIT
        curl -fsSL "{url}" | tar -xJ -C "$tmpdir"
        inner=$(ls "$tmpdir" | head -1)
        if [ -f "$tmpdir/$inner/PupkitShell" ]; then
            cp "$tmpdir/$inner/PupkitShell" "{target}"
            chmod +x "{target}"
            if [ -d "$tmpdir/$inner/PupkitShell_PupkitShell.bundle" ]; then
                rm -rf "{bundle}"
                cp -R "$tmpdir/$inner/PupkitShell_PupkitShell.bundle" "{bundle}"
            fi
            echo "PupkitShell updated"
        else
            echo "PupkitShell not found in archive (non-fatal)" >&2
        fi
        "#,
        url = archive_url,
        target = shell_target.display(),
        bundle = bundle_target.display(),
    );

    let status = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("failed to run PupkitShell update: {error}"))?;

    if !status.success() {
        return Err("PupkitShell download failed".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        LatestRelease, VersionOrdering, cargo_home_bin_dir_from, compare_versions,
        is_homebrew_install, is_source_build_path, normalize_version, parse_latest_release,
        parse_semver_core, plan_update,
    };
    use std::path::{Path, PathBuf};

    fn path(value: &str) -> PathBuf {
        PathBuf::from(value)
    }

    #[test]
    fn cargo_home_defaults_to_home_directory() {
        assert_eq!(
            cargo_home_bin_dir_from(None, Some(path("/tmp/pupkit-home")), None).unwrap(),
            path("/tmp/pupkit-home/.cargo/bin")
        );
    }

    #[test]
    fn relative_cargo_home_uses_current_directory() {
        assert_eq!(
            cargo_home_bin_dir_from(
                Some(path(".cargo-alt")),
                Some(path("/tmp/ignored-home")),
                Some(path("/worktree"))
            )
            .unwrap(),
            path("/worktree/.cargo-alt/bin")
        );
    }

    #[test]
    fn detects_homebrew_layouts() {
        assert!(is_homebrew_install(Path::new(
            "/opt/homebrew/Cellar/pupkit/0.0.3/bin/pupkit"
        )));
        assert!(is_homebrew_install(Path::new(
            "/home/linuxbrew/.linuxbrew/bin/pupkit"
        )));
        assert!(!is_homebrew_install(Path::new("/usr/local/bin/pupkit")));
    }

    #[test]
    fn detects_source_build_layouts() {
        assert!(is_source_build_path(Path::new(
            "/repo/target/release/pupkit"
        )));
        assert!(is_source_build_path(Path::new("/repo/target/debug/pupkit")));
        assert!(!is_source_build_path(Path::new(
            "/home/user/.cargo/bin/pupkit"
        )));
    }

    #[test]
    fn update_plan_rejects_homebrew_installs() {
        let error = plan_update(
            Path::new("/opt/homebrew/Cellar/pupkit/0.0.3/bin/pupkit"),
            Path::new("/home/user/.cargo/bin/pupkit"),
        )
        .unwrap_err();

        assert!(error.contains("brew upgrade pupkit"));
    }

    #[test]
    fn update_plan_rejects_source_builds() {
        let error = plan_update(
            Path::new("/repo/target/release/pupkit"),
            Path::new("/home/user/.cargo/bin/pupkit"),
        )
        .unwrap_err();

        assert!(error.contains("rebuild from source"));
    }

    #[test]
    fn update_plan_skips_sync_for_shell_installer_path() {
        let plan = plan_update(
            Path::new("/home/user/.cargo/bin/pupkit"),
            Path::new("/home/user/.cargo/bin/pupkit"),
        )
        .unwrap();

        assert!(plan.sync_target.is_none());
    }

    #[test]
    fn update_plan_syncs_manual_install_locations() {
        let plan = plan_update(
            Path::new("/home/user/.local/bin/pupkit"),
            Path::new("/home/user/.cargo/bin/pupkit"),
        )
        .unwrap();

        assert_eq!(plan.sync_target, Some(path("/home/user/.local/bin/pupkit")));
    }

    #[test]
    fn parses_latest_release_tag_name() {
        let release = parse_latest_release(r#"{"tag_name":"v0.0.5"}"#).unwrap();

        assert_eq!(
            release,
            LatestRelease {
                tag_name: "v0.0.5".to_string(),
                version: "0.0.5".to_string(),
            }
        );
    }

    #[test]
    fn normalize_version_strips_leading_v() {
        assert_eq!(normalize_version("v0.0.5").unwrap(), "0.0.5");
        assert_eq!(normalize_version("0.0.5").unwrap(), "0.0.5");
        assert!(normalize_version("latest").is_none());
    }

    #[test]
    fn parses_semver_core_triplet() {
        assert_eq!(parse_semver_core("0.0.5").unwrap(), (0, 0, 5));
        assert_eq!(parse_semver_core("0.0.5-beta.1").unwrap(), (0, 0, 5));
        assert!(parse_semver_core("0.0").is_none());
    }

    #[test]
    fn compares_versions_correctly() {
        assert_eq!(
            compare_versions("0.0.4", "0.0.5"),
            VersionOrdering::CurrentIsOlder
        );
        assert_eq!(compare_versions("0.0.5", "0.0.5"), VersionOrdering::Equal);
        assert_eq!(
            compare_versions("0.0.6", "0.0.5"),
            VersionOrdering::CurrentIsNewer
        );
        assert_eq!(compare_versions("dev", "0.0.5"), VersionOrdering::Unknown);
    }
}
