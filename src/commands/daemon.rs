use std::time::Duration;

use crate::daemon::{DaemonServer, PupkitDaemon, shell_launcher, watcher};

pub fn execute() -> Result<(), String> {
    let mut daemon = PupkitDaemon::bootstrap();
    let config = daemon.config().clone();
    println!("{}", daemon.report());

    let server = DaemonServer::new(daemon, Duration::from_secs(300));

    // 1. Bind socket (must succeed before launching anything)
    let listener = server.bind(&config.socket_path)?;

    // 2. Start file watcher for auto-discovering AI sessions
    if let Some(home) = std::env::var_os("HOME").map(Into::into) {
        watcher::spawn_watcher(server.daemon_arc(), home);
    }

    // 3. Launch PupkitShell GUI (macOS only, non-blocking)
    if let Some(ref shell_path) = config.shell_binary_path {
        shell_launcher::try_launch(shell_path);
    }

    // 4. Accept connections (blocking)
    server.accept_loop(listener)
}
