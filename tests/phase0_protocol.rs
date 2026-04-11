use pupkit::daemon::PupkitDaemon;
use pupkit::protocol::{
    ApprovalBehavior, HookDecision, RequestId, SessionEvent, SessionEventKind, SessionEventPayload,
    SessionId, SessionSnapshot, SessionStatus, SourceKind, UiAction,
};

#[test]
fn protocol_roundtrip_keeps_session_identity() {
    let event = SessionEvent::new(
        SourceKind::ClaudeCode,
        SessionId::new("session-123"),
        SessionEventKind::SessionStarted,
    )
    .with_title("demo")
    .with_summary("running");

    let json = serde_json::to_string(&event).unwrap();
    let restored: SessionEvent = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.session_id.as_str(), "session-123");
    assert_eq!(restored.kind, SessionEventKind::SessionStarted);
    assert_eq!(restored.title.as_deref(), Some("demo"));
}

#[test]
fn session_snapshot_marks_attention_states() {
    let snapshot = SessionSnapshot::new(
        SessionId::new("session-456"),
        SourceKind::Codex,
        "demo task".to_string(),
        SessionStatus::WaitingApproval,
    );

    assert!(snapshot.status.requires_attention());
}

#[test]
fn hook_decision_serializes_approval_behavior() {
    let decision = HookDecision::Approval {
        request_id: RequestId::new("req-1"),
        behavior: ApprovalBehavior::Allow,
    };

    let json = serde_json::to_string(&decision).unwrap();

    assert!(json.contains("Allow"));
}

#[test]
fn daemon_ingests_and_resolves_approval_flow() {
    let mut daemon = PupkitDaemon::bootstrap();
    daemon
        .ingest_event(
            SessionEvent::new(
                SourceKind::ClaudeCode,
                SessionId::new("session-approval"),
                SessionEventKind::ApprovalRequested,
            )
            .with_title("approval")
            .with_payload(SessionEventPayload::ApprovalRequest {
                request_id: RequestId::new("req-1"),
                tool_name: "Edit".to_string(),
                tool_input_summary: "modify src/main.rs".to_string(),
            }),
        )
        .unwrap();

    let decision = daemon
        .apply_ui_action(UiAction::Approve {
            request_id: RequestId::new("req-1"),
            always: false,
        })
        .unwrap()
        .unwrap();

    assert!(matches!(
        decision,
        HookDecision::Approval {
            behavior: ApprovalBehavior::Allow,
            ..
        }
    ));
}
