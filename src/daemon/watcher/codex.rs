use std::path::Path;

use serde_json::Value;

use super::{extract_timestamp, session_id_from_path, PendingToolTracker, truncate_summary};
use crate::protocol::{
    SessionEvent, SessionEventKind, SessionId, SourceKind,
};

/// Parse a Codex JSONL line.
///
/// Codex has types: session_meta, event_msg, response_item, turn_context, compacted.
/// We generate:
/// - `session_meta` → SessionStarted
/// - `event_msg` with payload.type=="task_started" → SessionUpdated
pub(super) fn parse_codex_line(value: &Value, path: &Path) -> Option<SessionEvent> {
    let line_type = value.get("type")?.as_str()?;
    let occurred_at = extract_timestamp(value);

    match line_type {
        "session_meta" => {
            let payload = value.get("payload")?;
            // Always use filename-based session_id so all events from the same
            // file share the same ID (event_msg and track_codex_tool_calls
            // already use session_id_from_path).
            let session_id = session_id_from_path(path);
            let cwd = payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from);
            let model = payload
                .get("model_provider")
                .and_then(|v| v.as_str())
                .unwrap_or("Codex");

            let mut event = SessionEvent::new(
                SourceKind::Codex,
                SessionId::new(&session_id),
                SessionEventKind::SessionStarted,
            );
            event = event.with_title(format!("Codex ({model})"));
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd);
            }
            event = event.with_occurred_at(occurred_at);
            Some(event)
        }
        "event_msg" => {
            let payload = value.get("payload")?;
            let event_type = payload.get("type")?.as_str()?;
            if event_type == "task_started" || event_type == "task_completed" {
                let session_id = session_id_from_path(path);
                let kind = if event_type == "task_completed" {
                    SessionEventKind::SessionEnded
                } else {
                    SessionEventKind::SessionUpdated
                };
                let mut event = SessionEvent::new(
                    SourceKind::Codex,
                    SessionId::new(&session_id),
                    kind,
                );
                event = event.with_occurred_at(occurred_at);
                Some(event)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Codex approval-relevant tool names.
const CODEX_APPROVAL_TOOLS: &[&str] = &["exec_command", "apply_patch"];

/// Track Codex tool calls for approval detection.
///
/// Records `function_call` from response_item and clears on `function_call_output`
/// or `exec_command_end`.
pub(super) fn track_codex_tool_calls(value: &Value, path: &Path, tracker: &mut PendingToolTracker) {
    let line_type = match value.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return,
    };
    let session_id = session_id_from_path(path);

    match line_type {
        "response_item" => {
            let payload = match value.get("payload") {
                Some(p) => p,
                None => return,
            };
            let payload_type = match payload.get("type").and_then(|t| t.as_str()) {
                Some(t) => t,
                None => return,
            };
            match payload_type {
                "function_call" => {
                    let tool_name = match payload.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n,
                        None => return,
                    };
                    if !CODEX_APPROVAL_TOOLS.contains(&tool_name) {
                        return;
                    }
                    let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => return,
                    };
                    let summary = payload
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        .and_then(|args| {
                            args.get("cmd")
                                .or_else(|| args.get("command"))
                                .and_then(|v| v.as_str())
                                .map(|s| truncate_summary(s))
                        })
                        .unwrap_or_else(|| tool_name.to_string());

                    tracker.record_requested(
                        call_id,
                        session_id,
                        SourceKind::Codex,
                        tool_name.to_string(),
                        summary,
                        Some(path.to_path_buf()),
                    );
                }
                "function_call_output" => {
                    if let Some(call_id) = payload.get("call_id").and_then(|v| v.as_str()) {
                        tracker.mark_started(call_id);
                    }
                }
                "custom_tool_call" => {
                    if payload.get("status").and_then(|v| v.as_str()) == Some("completed") {
                        if let Some(call_id) = payload.get("call_id").and_then(|v| v.as_str()) {
                            tracker.mark_started(call_id);
                        }
                    }
                }
                _ => {}
            }
        }
        "event_msg" => {
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(|t| t.as_str()) == Some("exec_command_end") {
                    if let Some(call_id) = payload.get("call_id").and_then(|v| v.as_str()) {
                        tracker.mark_started(call_id);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_codex_session_meta_produces_session_started() {
        let line = r#"{"type":"session_meta","timestamp":"2026-03-20T07:34:51.922Z","payload":{"id":"codex-sess-1","cwd":"/tmp/project","model_provider":"openai"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_codex_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        // session_id comes from the filename, not payload.id
        assert_eq!(event.session_id.as_str(), "test");
        assert_eq!(event.cwd.as_deref(), Some("/tmp/project"));
        assert!(event.title.unwrap().contains("Codex"));
    }

    #[test]
    fn parse_codex_task_started_produces_session_updated() {
        let line = r#"{"type":"event_msg","timestamp":"2026-03-20T07:35:00.000Z","payload":{"type":"task_started","turn_id":"turn-1"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_codex_line(&value, Path::new("codex-sess-1.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
    }

    #[test]
    fn parse_codex_task_completed_produces_session_ended() {
        let line = r#"{"type":"event_msg","timestamp":"2026-03-20T08:00:00.000Z","payload":{"type":"task_completed"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_codex_line(&value, Path::new("codex-sess-1.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionEnded);
    }

    #[test]
    fn codex_function_call_records_pending_and_output_clears() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/2026/04/08/rollout-sess1.jsonl");

        let call_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "call_id": "call_codex_1",
                "arguments": "{\"cmd\":\"pwd\"}"
            }
        }"#;
        let value: Value = serde_json::from_str(call_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);
        assert!(tracker.pending.contains_key("call_codex_1"));
        assert_eq!(tracker.pending["call_codex_1"].source, SourceKind::Codex);

        let output_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": "call_codex_1",
                "output": "/Users/test"
            }
        }"#;
        let value: Value = serde_json::from_str(output_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn codex_exec_command_end_also_clears_pending() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/rollout-sess2.jsonl");

        let call_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "call_id": "call_end_test",
                "arguments": "{\"cmd\":\"echo hi\"}"
            }
        }"#;
        let value: Value = serde_json::from_str(call_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);

        let end_json = r#"{
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "call_id": "call_end_test",
                "exit_code": 0,
                "status": "completed"
            }
        }"#;
        let value: Value = serde_json::from_str(end_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn codex_stale_function_call_becomes_approval() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/rollout-sess3.jsonl");

        let json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "call_id": "call_stale_codex",
                "arguments": "{\"cmd\":\"dangerous-cmd\"}"
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);

        let _ = tracker.advance_poll();
        let _ = tracker.advance_poll();
        let approvals = tracker.advance_poll();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].1, SourceKind::Codex);
        assert_eq!(approvals[0].2, "exec_command");
    }

    #[test]
    fn codex_completed_custom_tool_call_is_auto_cleared() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/rollout-sess4.jsonl");

        let call_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "apply_patch",
                "call_id": "call_patch_1",
                "arguments": "{}"
            }
        }"#;
        let value: Value = serde_json::from_str(call_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);

        let completed_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "custom_tool_call",
                "status": "completed",
                "call_id": "call_patch_1",
                "name": "apply_patch"
            }
        }"#;
        let value: Value = serde_json::from_str(completed_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }
}
