use std::env;
use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const INSTALLER_URL: &str =
    "https://github.com/pupkit-labs/pupkit-cli/releases/latest/download/pupkit-installer.sh";

pub fn execute() -> Result<(), String> {
    let current_exe = env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let binary_name = current_exe
        .file_name()
        .ok_or_else(|| "failed to determine current executable name".to_string())?;
    let install_bin = shell_installer_bin_path(binary_name)?;
    let plan = plan_update(&current_exe, &install_bin)?;

    run_shell_installer()?;

    if let Some(sync_target) = plan.sync_target {
        sync_installed_binary(&plan.install_bin, &sync_target)?;
        println!("pupkit updated and synced to {}", sync_target.display());
    } else {
        println!("pupkit updated successfully.");
    }

    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UpdatePlan {
    install_bin: PathBuf,
    sync_target: Option<PathBuf>,
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

#[cfg(test)]
mod tests {
    use super::{cargo_home_bin_dir_from, is_homebrew_install, is_source_build_path, plan_update};
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
}
