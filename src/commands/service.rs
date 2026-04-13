use std::time::Duration;

use crate::daemon::shell_launcher;

use super::daemon;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceCommand {
    Start,
    Stop,
    Restart,
    Status,
}

pub fn execute(cmd: ServiceCommand) -> Result<(), String> {
    match cmd {
        ServiceCommand::Start => execute_start(),
        ServiceCommand::Stop => execute_stop(),
        ServiceCommand::Restart => execute_restart(),
        ServiceCommand::Status => execute_status(),
    }
}

fn execute_start() -> Result<(), String> {
    let (running, _) = daemon::daemon_status();
    if running {
        // Daemon is up — make sure shell is also alive
        if !shell_launcher::is_running() {
            // Clear paused marker (user intent: start everything)
            clear_shell_paused_marker();
            let path = resolve_shell_path_best_effort();
            if let Some(ref p) = path {
                shell_launcher::try_launch(p);
                std::thread::sleep(Duration::from_millis(300));
            }
            if shell_launcher::is_running() {
                println!("pupkit shell restarted");
            } else {
                println!("pupkit is running (shell failed to start)");
            }
        } else {
            println!("pupkit is already running");
        }
        return Ok(());
    }

    daemon::spawn_daemon_background()?;

    // Wait for daemon to become ready (socket bind)
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        let (up, _) = daemon::daemon_status();
        if up {
            println!("pupkit started");
            return Ok(());
        }
    }

    println!("pupkit daemon spawned (may still be initializing)");
    Ok(())
}

fn clear_shell_paused_marker() {
    if let Some(h) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        let marker = h.join(".local/share/pupkit/shell-paused");
        let _ = std::fs::remove_file(&marker);
    }
}

fn resolve_shell_path_best_effort() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let config = crate::daemon::DaemonConfig::default_for_home(Some(home));
    config.shell_binary_path.or_else(|| shell_launcher::ensure_available())
}

fn execute_stop() -> Result<(), String> {
    let _ = shell_launcher::stop_shell();
    // Don't write paused marker here — daemon is going down anyway,
    // and next `start` should bring everything back fresh
    daemon::stop_daemon(true)?;
    println!("pupkit stopped");
    Ok(())
}

fn execute_restart() -> Result<(), String> {
    let _ = shell_launcher::stop_shell();
    let _ = daemon::stop_daemon(false);
    // Clear marker so fresh daemon start brings shell back
    clear_shell_paused_marker();
    std::thread::sleep(Duration::from_millis(500));
    execute_start()
}

fn execute_status() -> Result<(), String> {
    let (daemon_running, pid_info) = daemon::daemon_status();
    let shell_running = shell_launcher::is_running();

    let status = match (daemon_running, shell_running) {
        (true, true) => "running",
        (true, false) => "running (shell stopped)",
        (false, true) => "stopped (orphan shell)",
        (false, false) => "stopped",
    };

    println!("pupkit:  {}", match (daemon_running, pid_info) {
        (true, Some(pid)) => format!("{status} (pid: {pid})"),
        _ => status.to_string(),
    });
    println!("  daemon:  {}", if daemon_running { "up" } else { "down" });
    println!("  shell:   {}", if shell_running { "up" } else { "down" });

    Ok(())
}
