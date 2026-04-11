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
