use std::path::Path;

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

#[cfg(not(target_os = "macos"))]
pub fn try_launch(_binary_path: &Path) {
    // No-op on non-macOS platforms
}
