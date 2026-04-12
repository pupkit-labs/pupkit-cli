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
/// Records tool requests from `assistant.message` for approval-needing tools,
/// and marks them as started when `tool.execution_start` is seen.
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
        "assistant.message" => {
            let data = match value.get("data") {
                Some(d) => d,
                None => return,
            };
            if let Some(trs) = data.get("toolRequests").and_then(|v| v.as_array()) {
                for tr in trs {
                    let tool_name = match tr.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n,
                        None => continue,
                    };
                    if !APPROVAL_TOOLS.contains(&tool_name) {
                        continue;
                    }
                    let tool_call_id = match tr.get("toolCallId").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => continue,
                    };
                    let summary = tr
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
                        session_id.clone(),
                        SourceKind::Copilot,
                        tool_name.to_string(),
                        summary,
                        None,
                    );
                }
            }
        }
        "tool.execution_start" => {
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
}
