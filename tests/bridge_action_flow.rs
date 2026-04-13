use pupkit::daemon::{DaemonConfig, DaemonServer, PupkitDaemon};
use std::os::unix::net::UnixListener;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_config(name: &str) -> DaemonConfig {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() % 1_000_000;
    let root = std::env::temp_dir().join(format!("pk-{name}-{ts}"));
    DaemonConfig {
        socket_path: root.join("d.sock"),
        state_path: root.join("state.json"),
        pid_path: root.join("d.pid"),
        shell_paused_path: root.join("shell-paused"),
        shell_binary_path: None,
    }
}

#[test]
fn daemon_server_rejects_second_bind_on_live_socket() {
    let config = temp_config("bind");
    if let Some(parent) = config.socket_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let _listener = UnixListener::bind(&config.socket_path).unwrap();

    let server = DaemonServer::new(
        PupkitDaemon::for_config(config.clone()),
        Duration::from_secs(2),
    );
    let err = server
        .serve_forever(config.socket_path.as_path())
        .unwrap_err();
    assert!(err.contains("already active"));

    let _ = std::fs::remove_file(config.socket_path);
}
