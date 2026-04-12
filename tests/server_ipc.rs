use pupkit::daemon::{DaemonConfig, DaemonServer, PupkitDaemon};
use pupkit::protocol::{
    ApprovalBehavior, ClientRequest, HookEnvelope, RequestId, ServerResponse, SessionEvent,
    SessionEventKind, SessionEventPayload, SessionId, SourceKind, UiAction,
};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn temp_config(name: &str) -> DaemonConfig {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("pupkit-server-{name}-{ts}-{}", std::process::id()));
    DaemonConfig {
        socket_path: root.join("pupkitd.sock"),
        state_path: root.join("daemon-state.json"),
    }
}

#[test]
fn ui_action_unblocks_blocking_hook_request() {
    let daemon = PupkitDaemon::for_config(temp_config("ipc"));
    let server = DaemonServer::new(daemon, Duration::from_secs(2));
    let hook_server = server.clone();

    let handle = std::thread::spawn(move || {
        hook_server
            .handle_client_request(ClientRequest::Hook(HookEnvelope {
                event: SessionEvent::new(
                    SourceKind::ClaudeCode,
                    SessionId::new("session-1"),
                    SessionEventKind::ApprovalRequested,
                )
                .with_title("demo")
                .with_payload(SessionEventPayload::ApprovalRequest {
                    request_id: RequestId::new("req-1"),
                    tool_name: "Edit".to_string(),
                    tool_input_summary: "update src/lib.rs".to_string(),
                }),
                expects_response: true,
            }))
            .unwrap()
    });

    std::thread::sleep(Duration::from_millis(100));

    let ui_response = server
        .handle_client_request(ClientRequest::Ui(UiAction::Approve {
            request_id: RequestId::new("req-1"),
            always: false,
        }))
        .unwrap();
    assert!(matches!(ui_response, ServerResponse::UiActionResult { .. }));

    let hook_response = handle.join().unwrap();
    assert!(matches!(
        hook_response,
        ServerResponse::HookDecision(pupkit::protocol::HookDecision::Approval {
            behavior: ApprovalBehavior::Allow,
            ..
        })
    ));
}

#[test]
fn timeout_clears_waiting_attention_state() {
    let daemon = PupkitDaemon::for_config(temp_config("timeout"));
    let server = DaemonServer::new(daemon, Duration::from_millis(50));

    let response = server
        .handle_client_request(ClientRequest::Hook(HookEnvelope {
            event: SessionEvent::new(
                SourceKind::ClaudeCode,
                SessionId::new("session-timeout"),
                SessionEventKind::ApprovalRequested,
            )
            .with_title("demo")
            .with_payload(SessionEventPayload::ApprovalRequest {
                request_id: RequestId::new("req-timeout"),
                tool_name: "Edit".to_string(),
                tool_input_summary: "update src/lib.rs".to_string(),
            }),
            expects_response: true,
        }))
        .unwrap();

    assert!(matches!(
        response,
        ServerResponse::HookDecision(pupkit::protocol::HookDecision::Timeout { .. })
    ));

    let state = server
        .handle_client_request(ClientRequest::StateSnapshot)
        .unwrap();
    match state {
        ServerResponse::StateSnapshot(snapshot) => {
            assert!(snapshot.attentions.is_empty());
        }
        other => panic!("unexpected state response: {other:?}"),
    }
}

#[test]
fn malformed_stream_returns_structured_error_response() {
    let daemon = PupkitDaemon::for_config(temp_config("bad-json"));
    let server = DaemonServer::new(daemon, Duration::from_secs(1));
    let (client, server_stream) = UnixStream::pair().unwrap();

    let handle = std::thread::spawn(move || {
        server.serve_stream(server_stream).unwrap();
    });

    use std::io::{Read, Write};
    let mut client = client;
    client.write_all(b"not json").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    let mut buf = String::new();
    client.read_to_string(&mut buf).unwrap();
    handle.join().unwrap();

    assert!(buf.contains("Error"));
}
