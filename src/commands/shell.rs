use std::path::PathBuf;

use crate::daemon::{DaemonConfig, shell_launcher};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellCommand {
    Start,
    Stop,
    Restart,
    Status,
}

pub fn execute(cmd: ShellCommand) -> Result<(), String> {
    match cmd {
        ShellCommand::Start => execute_start(),
        ShellCommand::Stop => execute_stop(),
        ShellCommand::Restart => execute_restart(),
        ShellCommand::Status => execute_status(),
    }
}

fn execute_start() -> Result<(), String> {
    if shell_launcher::is_running() {
        println!("PupkitShell is already running");
        return Ok(());
    }

    // Remove paused marker so watchdog can also keep it alive
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(ref h) = home {
        let marker = DaemonConfig::default_for_home(Some(h.clone())).shell_paused_path;
        let _ = std::fs::remove_file(&marker);
    }

    let path = resolve_shell_path()?;
    shell_launcher::try_launch(&path);

    // Brief wait for process to register with pgrep
    std::thread::sleep(std::time::Duration::from_millis(300));

    if shell_launcher::is_running() {
        println!("PupkitShell started");
    } else {
        return Err("failed to start PupkitShell".to_string());
    }
    Ok(())
}

fn execute_stop() -> Result<(), String> {
    // Write paused marker so watchdog won't restart
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(ref h) = home {
        let marker = DaemonConfig::default_for_home(Some(h.clone())).shell_paused_path;
        if let Some(parent) = marker.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::File::create(&marker);
    }

    shell_launcher::stop_shell()?;
    println!("PupkitShell stopped");
    Ok(())
}

fn execute_restart() -> Result<(), String> {
    if shell_launcher::is_running() {
        shell_launcher::stop_shell()?;
        // Wait briefly for process to exit
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    execute_start()
}

fn execute_status() -> Result<(), String> {
    if shell_launcher::is_running() {
        println!("PupkitShell: running");
    } else {
        println!("PupkitShell: stopped");
    }
    Ok(())
}

fn resolve_shell_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME not set")?;
    let config = DaemonConfig::default_for_home(Some(home));

    if let Some(path) = config.shell_binary_path {
        return Ok(path);
    }
    if let Some(path) = shell_launcher::ensure_available() {
        return Ok(path);
    }
    Err("PupkitShell binary not found".to_string())
}
