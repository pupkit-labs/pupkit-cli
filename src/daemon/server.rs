use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::daemon::PupkitDaemon;
use crate::daemon::watcher;
use crate::protocol::{
    ClientRequest, HookDecision, ServerResponse, SessionEventKind, SessionStatus,
};

#[derive(Clone)]
pub struct DaemonServer {
    daemon: Arc<Mutex<PupkitDaemon>>,
    request_timeout: Duration,
}

impl DaemonServer {
    pub fn new(daemon: PupkitDaemon, request_timeout: Duration) -> Self {
        Self {
            daemon: Arc::new(Mutex::new(daemon)),
            request_timeout,
        }
    }

    pub fn handle_client_request(&self, request: ClientRequest) -> Result<ServerResponse, String> {
        match request {
            ClientRequest::Hook(envelope) => {
                let blocking = envelope.expects_response
                    && matches!(
                        envelope.event.kind,
                        SessionEventKind::ApprovalRequested | SessionEventKind::QuestionRequested
                    );

                if blocking {
                    let waiter = self
                        .daemon
                        .lock()
                        .unwrap()
                        .ingest_blocking_event(envelope.event)?;
                    let decision = waiter.wait_for_decision(self.request_timeout);

                    if let HookDecision::Timeout { request_id }
                    | HookDecision::Cancelled { request_id } = &decision
                    {
                        self.daemon
                            .lock()
                            .unwrap()
                            .cleanup_request(request_id, SessionStatus::Running)?;
                    }

                    Ok(ServerResponse::HookDecision(decision))
                } else {
                    self.daemon.lock().unwrap().ingest_event(envelope.event)?;
                    Ok(ServerResponse::Ack)
                }
            }
            ClientRequest::Ui(action) => {
                let mut daemon = self.daemon.lock().unwrap();
                let decision = daemon.apply_ui_action(action)?;
                let state = daemon.state_snapshot();
                Ok(ServerResponse::UiActionResult { decision, state })
            }
            ClientRequest::StateSnapshot => {
                let state = self.daemon.lock().unwrap().state_snapshot();
                Ok(ServerResponse::StateSnapshot(state))
            }
        }
    }

    pub fn serve_stream(&self, mut stream: UnixStream) -> Result<(), String> {
        let mut request_body = String::new();
        let response = match stream.read_to_string(&mut request_body) {
            Ok(_) => match serde_json::from_str::<ClientRequest>(&request_body) {
                Ok(request) => match self.handle_client_request(request) {
                    Ok(response) => response,
                    Err(error) => ServerResponse::Error { message: error },
                },
                Err(error) => ServerResponse::Error {
                    message: format!("failed to parse client request: {error}"),
                },
            },
            Err(error) => ServerResponse::Error {
                message: format!("failed to read client request: {error}"),
            },
        };

        let response_body = serde_json::to_string(&response)
            .map_err(|error| format!("failed to serialize server response: {error}"))?;
        stream
            .write_all(response_body.as_bytes())
            .map_err(|error| format!("failed to write server response: {error}"))?;
        Ok(())
    }

    pub fn serve_forever(&self, socket_path: &Path) -> Result<(), String> {
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create socket dir: {error}"))?;
        }
        if socket_path.exists() {
            if UnixStream::connect(socket_path).is_ok() {
                return Err(format!(
                    "refusing to start: daemon socket already active at {}",
                    socket_path.display()
                ));
            }
            let _ = fs::remove_file(socket_path);
        }

        let listener = UnixListener::bind(socket_path)
            .map_err(|error| format!("failed to bind unix socket: {error}"))?;

        // Start file watcher for auto-discovering AI sessions
        if let Some(home) = env::var_os("HOME").map(Into::into) {
            watcher::spawn_watcher(self.daemon.clone(), home);
        }

        for stream in listener.incoming() {
            let server = self.clone();
            match stream {
                Ok(stream) => {
                    thread::spawn(move || {
                        let _ = server.serve_stream(stream);
                    });
                }
                Err(error) => {
                    return Err(format!("failed to accept unix socket connection: {error}"));
                }
            }
        }
        Ok(())
    }
}
