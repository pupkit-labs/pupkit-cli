use std::time::Duration;

use crate::daemon::{DaemonServer, PupkitDaemon};

pub fn execute() -> Result<(), String> {
    let daemon = PupkitDaemon::bootstrap();
    let socket_path = daemon.config().socket_path.clone();
    println!("{}", daemon.report());
    let server = DaemonServer::new(daemon, Duration::from_secs(300));
    server.serve_forever(socket_path.as_path())
}
