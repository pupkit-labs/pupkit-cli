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

    if is_running() {
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
pub fn is_running() -> bool {
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

#[cfg(not(target_os = "macos"))]
pub fn is_running() -> bool {
    false
}

/// Spawns a background thread that periodically checks if PupkitShell is alive
/// and relaunches it if it has exited. Respects the shell-paused marker file.
#[cfg(target_os = "macos")]
pub fn spawn_watchdog(binary_path: PathBuf) {
    use std::thread;
    use std::time::Duration;

    let paused_path = std::env::var_os("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".local/share/pupkit/shell-paused"));

    thread::Builder::new()
        .name("shell-watchdog".into())
        .spawn(move || {
            // Wait a bit before first check to let initial launch settle
            thread::sleep(Duration::from_secs(15));
            loop {
                thread::sleep(Duration::from_secs(10));
                // Skip if user explicitly paused the shell
                if paused_path.as_ref().is_some_and(|p| p.exists()) {
                    continue;
                }
                if !is_running() {
                    eprintln!("[pupkit] PupkitShell not running; watchdog restarting...");
                    try_launch(&binary_path);
                }
            }
        })
        .ok();
}

#[cfg(not(target_os = "macos"))]
pub fn spawn_watchdog(_binary_path: PathBuf) {}

/// Stops all running PupkitShell processes.
#[cfg(target_os = "macos")]
pub fn stop_shell() -> Result<(), String> {
    use std::process::{Command, Stdio};
    let output = Command::new("pgrep")
        .args(["-x", "PupkitShell"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("pgrep failed: {e}"))?;

    let pids: Vec<&str> = std::str::from_utf8(&output.stdout)
        .unwrap_or("")
        .lines()
        .filter(|l| !l.is_empty())
        .collect();

    if pids.is_empty() {
        return Err("PupkitShell is not running".to_string());
    }

    for pid in &pids {
        let _ = Command::new("kill")
            .arg(pid.trim())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn stop_shell() -> Result<(), String> {
    Err("PupkitShell is only available on macOS".to_string())
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
