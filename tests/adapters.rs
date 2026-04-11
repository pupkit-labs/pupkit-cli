use pupkit::adapters::claude::normalize_claude_event;
use pupkit::adapters::codex::normalize_codex_event;
use pupkit::protocol::SessionEventKind;
use serde_json::json;

#[test]
fn claude_adapter_normalizes_permission_request() {
    let event = normalize_claude_event(&json!({
        "session_id": "claude-session",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Edit",
        "tool_input": {"path": "src/lib.rs"}
    }))
    .unwrap();

    assert_eq!(event.kind, SessionEventKind::ApprovalRequested);
}

#[test]
fn codex_adapter_normalizes_completion() {
    let event = normalize_codex_event(&json!({
        "session_id": "codex-session",
        "event": "task.completed",
        "summary": "done",
        "reason": "tests passed"
    }))
    .unwrap();

    assert_eq!(event.kind, SessionEventKind::CompletionPublished);
}

#[test]
fn fallback_request_ids_are_distinct_for_distinct_inputs() {
    let first = normalize_claude_event(&json!({
        "session_id": "claude-session",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Edit",
        "tool_input": {"path": "src/one.rs"}
    }))
    .unwrap();
    let second = normalize_claude_event(&json!({
        "session_id": "claude-session",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Edit",
        "tool_input": {"path": "src/two.rs"}
    }))
    .unwrap();

    let first_id = match first.payload {
        pupkit::protocol::SessionEventPayload::ApprovalRequest { request_id, .. } => request_id,
        other => panic!("unexpected payload: {other:?}"),
    };
    let second_id = match second.payload {
        pupkit::protocol::SessionEventPayload::ApprovalRequest { request_id, .. } => request_id,
        other => panic!("unexpected payload: {other:?}"),
    };

    assert_ne!(first_id, second_id);
}
