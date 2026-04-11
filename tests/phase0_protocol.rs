use pupkit::protocol::{
    ApprovalBehavior, HookDecision, SessionEvent, SessionEventKind, SessionId, SessionSnapshot,
    SessionStatus, SourceKind,
};

#[test]
fn protocol_roundtrip_keeps_session_identity() {
    let event = SessionEvent::new(
        SourceKind::ClaudeCode,
        SessionId::new("session-123"),
        SessionEventKind::SessionStarted,
    );

    let json = serde_json::to_string(&event).unwrap();
    let restored: SessionEvent = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.session_id.as_str(), "session-123");
    assert_eq!(restored.kind, SessionEventKind::SessionStarted);
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
        behavior: ApprovalBehavior::Allow,
    };

    let json = serde_json::to_string(&decision).unwrap();

    assert!(json.contains("Allow"));
}
