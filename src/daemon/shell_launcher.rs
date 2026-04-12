use std::path::{Path, PathBuf};

/// Try to ensure PupkitShell is available. If not found, download from latest release.
/// Returns the path to PupkitShell binary if available.
#[cfg(target_os = "macos")]
pub fn ensure_available() -> Option<PathBuf> {
    // Already resolved by DaemonConfig
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("PupkitShell");
            if candidate.is_file() {
                return Some(candidate);
            }
            // Not found — try to download
            eprintln!("[pupkit] PupkitShell not found; downloading from latest release...");
            if download_shell(dir).is_ok() && candidate.is_file() {
                eprintln!("[pupkit] PupkitShell downloaded successfully");
                return Some(candidate);
            }
            eprintln!("[pupkit] PupkitShell download failed (non-fatal)");
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_available() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
pub fn try_launch(binary_path: &Path) {
    use std::process::{Command, Stdio};

    if !binary_path.is_file() {
        eprintln!("[pupkit] PupkitShell not found at {}", binary_path.display());
        return;
    }

    if is_shell_running() {
        eprintln!("[pupkit] PupkitShell already running; skipping launch");
        return;
    }

    match Command::new(binary_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            eprintln!("[pupkit] PupkitShell launched (pid: {})", child.id());
            // Detach: we don't wait on this child; let it run independently.
            std::mem::forget(child);
        }
        Err(error) => {
            eprintln!("[pupkit] failed to launch PupkitShell: {error}");
        }
    }
}

#[cfg(target_os = "macos")]
fn is_shell_running() -> bool {
    use std::process::{Command, Stdio};
    Command::new("pgrep")
        .args(["-x", "PupkitShell"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn download_shell(install_dir: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};

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
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("download failed: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("download script failed".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn try_launch(_binary_path: &Path) {
    // No-op on non-macOS platforms
}
