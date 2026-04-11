use pupkit::daemon::{DaemonConfig, DaemonServer, PupkitDaemon};
use pupkit::protocol::{
    ApprovalBehavior, ClientRequest, HookEnvelope, RequestId, ServerResponse, SessionEvent,
    SessionEventKind, SessionEventPayload, SessionId, SourceKind, UiAction,
};
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
