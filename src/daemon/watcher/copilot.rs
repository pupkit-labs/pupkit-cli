use std::path::Path;

use serde_json::Value;

use super::{
    extract_timestamp, session_id_from_copilot_path, truncate_summary, PendingToolTracker,
    APPROVAL_TOOLS,
};
use crate::protocol::{
    RequestId, SessionEvent, SessionEventKind, SessionEventPayload, SessionId, SourceKind,
};

/// Parse a Copilot JSONL line.
///
/// Copilot JSONL lives at `~/.copilot/session-state/<session-id>/events.jsonl`.
/// Types include session.start, assistant.turn_start, assistant.message, and tool.* events.
///
/// **Title handling**: only the `session.start` event sets a title (from `cwd`).
/// Other events must NOT set `.with_title(…)` because `app.rs` overwrites the session
/// title on every event that carries one — that would erase the project name.
pub(super) fn parse_copilot_line(value: &Value, path: &Path) -> Option<SessionEvent> {
    let line_type = value.get("type")?.as_str()?;
    let occurred_at = extract_timestamp(value);

    match line_type {
        "session.start" => {
            let session_id = session_id_from_copilot_path(path);
            let data = value.get("data")?;
            let cwd = data
                .get("context")
                .and_then(|c| c.get("cwd"))
                .and_then(|v| v.as_str());
            let title = cwd
                .and_then(|c| c.rsplit('/').next())
                .map(|dir| format!("Copilot · {dir}"))
                .unwrap_or_else(|| "Copilot Chat".to_string());
            let mut event = SessionEvent::new(
                SourceKind::Copilot,
                SessionId::new(&session_id),
                SessionEventKind::SessionStarted,
            )
            .with_title(title)
            .with_occurred_at(occurred_at);
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd.to_string());
            }
            Some(event)
        }
        "assistant.message" => {
            let session_id = session_id_from_copilot_path(path);
            let data = value.get("data")?;

            // Check for ask_user tool calls → QuestionRequested
            if let Some(tool_requests) = data.get("toolRequests").and_then(|v| v.as_array()) {
                for tr in tool_requests {
                    if tr.get("name").and_then(|n| n.as_str()) == Some("ask_user") {
                        let args = tr.get("arguments").unwrap_or(&Value::Null);
                        let question = args
                            .get("question")
                            .and_then(|q| q.as_str())
                            .unwrap_or("Question")
                            .to_string();
                        let choices: Vec<String> = args
                            .get("choices")
                            .and_then(|c| c.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let request_id = tr
                            .get("toolCallId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("copilot-ask");

                        let event = SessionEvent::new(
                            SourceKind::Copilot,
                            SessionId::new(&session_id),
                            SessionEventKind::QuestionRequested,
                        )
                        .with_summary(question.clone())
                        .with_payload(SessionEventPayload::QuestionRequest {
                            request_id: RequestId::new(request_id),
                            prompt: question,
                            options: choices,
                            allow_freeform: true,
                        })
                        .with_occurred_at(occurred_at);

                        return Some(event);
                    }
                }
            }

            // Regular assistant message → SessionUpdated
            let mut event = SessionEvent::new(
                SourceKind::Copilot,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            );
            event = event.with_occurred_at(occurred_at);
            if let Some(model) = data.get("model").and_then(|m| m.as_str()) {
                event = event.with_summary(format!("model: {model}"));
            }
            Some(event)
        }
        "assistant.turn_start" => {
            let session_id = session_id_from_copilot_path(path);
            let event = SessionEvent::new(
                SourceKind::Copilot,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            )
            .with_occurred_at(occurred_at);
            Some(event)
        }
        _ => None,
    }
}

/// Track Copilot tool calls for approval detection.
///
/// Copilot CLI writes `assistant.message` and `tool.execution_start` at the same
/// instant, but the actual approval prompt happens between `tool.execution_start`
/// and `tool.execution_complete`. So we track START→COMPLETE gaps:
///
/// - Record pending on `tool.execution_start` for approval-relevant tools
/// - Clear on `tool.execution_complete`
/// - If a tool call survives 2 poll cycles without completion, it's likely
///   waiting for user approval
pub(super) fn track_copilot_tool_calls(
    value: &Value,
    path: &Path,
    tracker: &mut PendingToolTracker,
) {
    let line_type = match value.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return,
    };

    let session_id = session_id_from_copilot_path(path);

    match line_type {
        "tool.execution_start" => {
            let data = match value.get("data") {
                Some(d) => d,
                None => return,
            };
            let tool_name = match data.get("toolName").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => return,
            };
            if !APPROVAL_TOOLS.contains(&tool_name) {
                return;
            }
            let tool_call_id = match data.get("toolCallId").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => return,
            };
            let summary = data
                .get("arguments")
                .and_then(|a| {
                    a.get("command")
                        .or_else(|| a.get("path"))
                        .or_else(|| a.get("file_text"))
                })
                .and_then(|v| v.as_str())
                .map(|s| truncate_summary(s))
                .unwrap_or_else(|| tool_name.to_string());

            tracker.record_requested(
                tool_call_id,
                session_id,
                SourceKind::Copilot,
                tool_name.to_string(),
                summary,
                Some(path.to_path_buf()),
            );
        }
        "tool.execution_complete" => {
            if let Some(tool_call_id) = value
                .get("data")
                .and_then(|d| d.get("toolCallId"))
                .and_then(|v| v.as_str())
            {
                tracker.mark_started(tool_call_id);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_copilot_turn_start_produces_session_updated() {
        let line = r#"{"type":"assistant.turn_start","timestamp":"2026-03-20T07:35:00.000Z","data":{}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/home/user/.copilot/session-state/abc-123/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
        assert_eq!(event.session_id.as_str(), "abc-123");
        assert_eq!(event.title, None);
    }

    #[test]
    fn parse_copilot_session_start_extracts_cwd_and_title() {
        let line = r#"{"type":"session.start","timestamp":"2026-04-01T02:57:21.058Z","data":{"sessionId":"c807f80f","copilotVersion":"1.0.14","startTime":"2026-04-01T02:57:21.032Z","context":{"cwd":"/Users/dev/projects/my-app"}}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/home/.copilot/session-state/c807f80f/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        assert_eq!(event.session_id.as_str(), "c807f80f");
        assert_eq!(event.title.as_deref(), Some("Copilot · my-app"));
        assert_eq!(event.cwd.as_deref(), Some("/Users/dev/projects/my-app"));
    }

    #[test]
    fn parse_copilot_session_start_without_cwd_uses_fallback_title() {
        let line = r#"{"type":"session.start","timestamp":"2026-04-01T02:57:21.058Z","data":{"sessionId":"abc"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/home/.copilot/session-state/abc/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        assert_eq!(event.title.as_deref(), Some("Copilot Chat"));
        assert_eq!(event.cwd, None);
    }

    #[test]
    fn parse_copilot_assistant_message_extracts_model() {
        let line = r#"{"type":"assistant.message","timestamp":"2026-03-20T07:35:00.000Z","data":{"model":"claude-sonnet-4.6"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/copilot/session-state/sess-x/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert!(event.summary.unwrap().contains("claude-sonnet-4.6"));
    }

    #[test]
    fn parse_copilot_ask_user_produces_question_requested() {
        let line = r#"{"type":"assistant.message","timestamp":"2026-03-20T07:35:00.000Z","data":{"toolRequests":[{"name":"ask_user","toolCallId":"call-123","arguments":{"question":"Which database?","choices":["PostgreSQL","MySQL","SQLite"]}}]}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/copilot/session-state/sess-ask/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::QuestionRequested);
        assert_eq!(event.session_id.as_str(), "sess-ask");
        if let SessionEventPayload::QuestionRequest {
            request_id,
            prompt,
            options,
            allow_freeform,
        } = &event.payload
        {
            assert_eq!(request_id.as_str(), "call-123");
            assert_eq!(prompt, "Which database?");
            assert_eq!(options, &["PostgreSQL", "MySQL", "SQLite"]);
            assert!(*allow_freeform);
        } else {
            panic!("expected QuestionRequest payload");
        }
    }

    #[test]
    fn copilot_session_start_emits_started_with_cwd() {
        let json = r#"{
            "type": "session.start",
            "data": {
                "sessionId": "abc-123",
                "copilotVersion": "1.0.14",
                "context": { "cwd": "/Users/test/my-project" }
            },
            "timestamp": "2026-04-01T02:57:21.058Z"
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        let path = Path::new("/home/.copilot/session-state/abc-123/events.jsonl");
        let event = parse_copilot_line(&value, path).expect("should parse session.start");
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        assert_eq!(event.source, SourceKind::Copilot);
        assert_eq!(event.cwd.as_deref(), Some("/Users/test/my-project"));
        assert_eq!(event.title.as_deref(), Some("Copilot · my-project"));
    }

    #[test]
    fn copilot_source_kind_is_copilot_not_unknown() {
        let json = r#"{
            "type": "assistant.turn_start",
            "data": {},
            "timestamp": "2026-04-01T03:00:00.000Z"
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        let path = Path::new("/home/.copilot/session-state/sess-1/events.jsonl");
        let event = parse_copilot_line(&value, path).expect("should parse turn_start");
        assert_eq!(event.source, SourceKind::Copilot);
    }

    // --- Copilot tool call tracking (START→COMPLETE) ---

    #[test]
    fn copilot_execution_start_records_pending_and_complete_clears() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.copilot/session-state/tool-sess/events.jsonl");

        let start_json = r#"{
            "type": "tool.execution_start",
            "data": {
                "toolCallId": "call-1",
                "toolName": "bash",
                "arguments": { "command": "cargo test" }
            }
        }"#;
        let value: Value = serde_json::from_str(start_json).unwrap();
        track_copilot_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);
        assert!(tracker.pending.contains_key("call-1"));
        assert_eq!(tracker.pending["call-1"].source, SourceKind::Copilot);
        assert!(tracker.pending["call-1"].summary.contains("cargo test"));

        let complete_json = r#"{
            "type": "tool.execution_complete",
            "data": {
                "toolCallId": "call-1"
            }
        }"#;
        let value: Value = serde_json::from_str(complete_json).unwrap();
        track_copilot_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn copilot_non_approval_tool_is_not_tracked() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.copilot/session-state/t-sess/events.jsonl");

        // read_bash is NOT in APPROVAL_TOOLS
        let json = r#"{
            "type": "tool.execution_start",
            "data": {
                "toolCallId": "call-read",
                "toolName": "read_bash",
                "arguments": {}
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_copilot_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn copilot_stale_execution_becomes_approval() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.copilot/session-state/stale-sess/events.jsonl");

        let json = r#"{
            "type": "tool.execution_start",
            "data": {
                "toolCallId": "call-stale",
                "toolName": "bash",
                "arguments": { "command": "rm -rf /tmp/demo.txt" }
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_copilot_tool_calls(&value, path, &mut tracker);

        // First advance: just seen, not yet stale
        let approvals = tracker.advance_poll();
        assert!(approvals.is_empty());

        // Second advance: stale → approval emitted
        let approvals = tracker.advance_poll();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].1, SourceKind::Copilot);
        assert_eq!(approvals[0].2, "bash");
        assert!(approvals[0].3.contains("rm -rf"));
    }

    #[test]
    fn copilot_quick_complete_prevents_approval() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.copilot/session-state/quick-sess/events.jsonl");

        // Tool starts
        let start = r#"{"type":"tool.execution_start","data":{"toolCallId":"call-q","toolName":"bash","arguments":{"command":"echo hi"}}}"#;
        let value: Value = serde_json::from_str(start).unwrap();
        track_copilot_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);

        // Tool completes before next poll
        let complete = r#"{"type":"tool.execution_complete","data":{"toolCallId":"call-q"}}"#;
        let value: Value = serde_json::from_str(complete).unwrap();
        track_copilot_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);

        // Advance poll — no approvals since it was already cleared
        let approvals = tracker.advance_poll();
        assert!(approvals.is_empty());
    }
}
