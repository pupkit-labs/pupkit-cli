use std::fs;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::daemon::{DaemonConfig, DaemonServer, PupkitDaemon, shell_launcher, watcher};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonCommand {
    Start,
    Stop,
    Restart,
    Status,
}

pub fn execute(cmd: DaemonCommand) -> Result<(), String> {
    match cmd {
        DaemonCommand::Start => execute_start(),
        DaemonCommand::Stop => execute_stop(),
        DaemonCommand::Restart => execute_restart(),
        DaemonCommand::Status => execute_status(),
    }
}

fn execute_start() -> Result<(), String> {
    let mut daemon = PupkitDaemon::bootstrap();
    let config = daemon.config().clone();
    println!("{}", daemon.report());

    let server = DaemonServer::new(daemon, Duration::from_secs(300));

    // 1. Bind socket (must succeed before launching anything)
    let listener = server.bind(&config.socket_path)?;

    // 2. Write PID file
    write_pid_file(&config.pid_path);

    // Clear shell-paused marker on fresh daemon start
    let _ = fs::remove_file(&config.shell_paused_path);

    // 3. Start file watcher for auto-discovering AI sessions
    if let Some(home) = std::env::var_os("HOME").map(Into::into) {
        watcher::spawn_watcher(server.daemon_arc(), home);
    }

    // 4. Launch PupkitShell GUI (macOS only, non-blocking)
    let shell_path = config
        .shell_binary_path
        .clone()
        .or_else(shell_launcher::ensure_available);
    if let Some(ref path) = shell_path {
        shell_launcher::try_launch(path);
        // 5. Start watchdog to keep PupkitShell alive
        shell_launcher::spawn_watchdog(path.clone());
    }

    // 6. Clean up PID file on exit (best-effort via drop guard)
    let _pid_guard = PidFileGuard(config.pid_path.clone());

    // 7. Accept connections (blocking)
    server.accept_loop(listener)
}

fn execute_stop() -> Result<(), String> {
    stop_daemon(true)
}

/// Stop the daemon process. Returns Ok if stopped or already not running (when quiet=true).
pub fn stop_daemon(strict: bool) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME not set")?;
    let config = DaemonConfig::default_for_home(Some(home));

    // Try to read PID file first
    if let Ok(contents) = fs::read_to_string(&config.pid_path) {
        if let Ok(pid) = contents.trim().parse::<u32>() {
            let status = Command::new("kill")
                .arg(pid.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            match status {
                Ok(s) if s.success() => {
                    let _ = fs::remove_file(&config.pid_path);
                    let _ = fs::remove_file(&config.socket_path);
                    println!("daemon stopped (pid: {pid})");
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    // Fallback: check socket
    if config.socket_path.exists() {
        let _ = fs::remove_file(&config.socket_path);
        let _ = fs::remove_file(&config.pid_path);
        println!("daemon socket cleaned up (process may have already exited)");
        return Ok(());
    }

    if strict {
        Err("daemon is not running".to_string())
    } else {
        Ok(())
    }
}

fn execute_restart() -> Result<(), String> {
    let _ = stop_daemon(false);
    std::thread::sleep(Duration::from_millis(500));
    execute_start()
}

fn execute_status() -> Result<(), String> {
    let (daemon_running, pid_info) = daemon_status();
    let shell_running = shell_launcher::is_running();

    println!("daemon:  {}", if daemon_running {
        match pid_info {
            Some(pid) => format!("running (pid: {pid})"),
            None => "running".to_string(),
        }
    } else {
        "stopped".to_string()
    });
    println!("shell:   {}", if shell_running { "running" } else { "stopped" });

    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(h) = home {
        let config = DaemonConfig::default_for_home(Some(h));
        println!("socket:  {}", config.socket_path.display());
    }

    Ok(())
}

/// Returns (is_running, optional_pid).
pub fn daemon_status() -> (bool, Option<u32>) {
    let home = match std::env::var_os("HOME").map(PathBuf::from) {
        Some(h) => h,
        None => return (false, None),
    };
    let config = DaemonConfig::default_for_home(Some(home));

    let daemon_running = config.socket_path.exists()
        && UnixStream::connect(&config.socket_path).is_ok();

    let pid_info = fs::read_to_string(&config.pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());

    (daemon_running, pid_info)
}

/// Spawn daemon as a detached background process (for `pupkit start`).
pub fn spawn_daemon_background() -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot resolve current executable: {e}"))?;

    match Command::new(&exe)
        .args(["daemon", "start"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            eprintln!("[pupkit] daemon started in background (pid: {})", child.id());
            std::mem::forget(child);
            Ok(())
        }
        Err(e) => Err(format!("failed to spawn daemon: {e}")),
    }
}

fn write_pid_file(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = fs::File::create(path) {
        let _ = write!(f, "{}", std::process::id());
    }
}

/// RAII guard that removes the PID file when dropped.
struct PidFileGuard(PathBuf);
impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
