use std::path::Path;

use serde_json::Value;

use super::{extract_assistant_summary, extract_timestamp, session_id_from_path, PendingToolTracker, truncate_summary};
use crate::protocol::{
    SessionEvent, SessionEventKind, SessionId, SourceKind,
};

/// Parse a Claude Code JSONL line into a SessionEvent.
///
/// Claude JSONL has types: user, assistant, system, progress, file-history-snapshot.
/// We generate:
/// - `user` (first in file) → SessionStarted
/// - `user` / `assistant` → SessionUpdated
/// - `user` with /exit content → SessionEnded
pub(super) fn parse_claude_line(value: &Value, path: &Path) -> Option<SessionEvent> {
    let line_type = value.get("type")?.as_str()?;
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| session_id_from_path(path));

    let cwd = value.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let occurred_at = extract_timestamp(value);

    match line_type {
        "user" => {
            let content = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if content.contains("/exit") || content.contains("command-name>/exit") {
                let mut event = SessionEvent::new(
                    SourceKind::ClaudeCode,
                    SessionId::new(&session_id),
                    SessionEventKind::SessionEnded,
                );
                if let Some(cwd) = cwd {
                    event = event.with_cwd(cwd);
                }
                event = event.with_occurred_at(occurred_at);
                return Some(event);
            }

            let mut event = SessionEvent::new(
                SourceKind::ClaudeCode,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            );
            if let Some(slug) = value.get("slug").and_then(|v| v.as_str()) {
                event = event.with_title(slug.replace('-', " "));
            }
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd);
            }
            event = event.with_occurred_at(occurred_at);
            Some(event)
        }
        "assistant" => {
            let mut event = SessionEvent::new(
                SourceKind::ClaudeCode,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            );
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd);
            }
            event = event.with_occurred_at(occurred_at);
            if let Some(summary) = extract_assistant_summary(value) {
                event = event.with_summary(summary);
            }
            Some(event)
        }
        _ => None,
    }
}

/// Approval-relevant tool names for Claude Code.
const CLAUDE_APPROVAL_TOOLS: &[&str] = &["Bash", "Write", "Edit", "MultiEdit"];

/// Track Claude Code tool calls for approval detection.
///
/// Records `tool_use` from assistant messages and clears them on `tool_result`.
pub(super) fn track_claude_tool_calls(value: &Value, path: &Path, tracker: &mut PendingToolTracker) {
    let line_type = match value.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return,
    };
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| session_id_from_path(path));

    match line_type {
        "assistant" => {
            let message = match value.get("message") {
                Some(m) => m,
                None => return,
            };
            if message.get("stop_reason").and_then(|v| v.as_str()) != Some("tool_use") {
                return;
            }
            if let Some(contents) = message.get("content").and_then(|c| c.as_array()) {
                for item in contents {
                    if item.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                        continue;
                    }
                    let tool_name = match item.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n,
                        None => continue,
                    };
                    if !CLAUDE_APPROVAL_TOOLS.contains(&tool_name) {
                        continue;
                    }
                    let tool_call_id = match item.get("id").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => continue,
                    };
                    let summary = item
                        .get("input")
                        .and_then(|inp| {
                            inp.get("command")
                                .or_else(|| inp.get("path"))
                                .or_else(|| inp.get("file_path"))
                        })
                        .and_then(|v| v.as_str())
                        .map(|s| truncate_summary(s))
                        .unwrap_or_else(|| tool_name.to_string());

                    tracker.record_requested(
                        tool_call_id,
                        session_id.clone(),
                        SourceKind::ClaudeCode,
                        tool_name.to_string(),
                        summary,
                        Some(path.to_path_buf()),
                    );
                }
            }
        }
        "user" => {
            if let Some(contents) = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in contents {
                    if item.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        if let Some(id) = item.get("tool_use_id").and_then(|v| v.as_str()) {
                            tracker.mark_started(id);
                        }
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
    fn parse_claude_user_line_produces_session_updated() {
        let line = r#"{"type":"user","sessionId":"sess-1","cwd":"/tmp","slug":"fix-bug","timestamp":1700000000,"message":{"role":"user","content":"hello"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_claude_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
        assert_eq!(event.session_id.as_str(), "sess-1");
        assert_eq!(event.title.as_deref(), Some("fix bug"));
        assert_eq!(event.cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn parse_claude_exit_produces_session_ended() {
        let line = r#"{"type":"user","sessionId":"sess-1","timestamp":1700000000,"message":{"role":"user","content":"<command-name>/exit</command-name>"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_claude_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionEnded);
    }

    #[test]
    fn parse_claude_assistant_line_extracts_summary() {
        let line = r#"{"type":"assistant","sessionId":"sess-1","timestamp":1700000000,"message":{"role":"assistant","content":"I'll help you fix the bug."}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_claude_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
        assert!(event.summary.unwrap().contains("fix the bug"));
    }

    #[test]
    fn parse_claude_system_line_returns_none() {
        let line = r#"{"type":"system","sessionId":"sess-1","subtype":"turn_duration","timestamp":1700000000}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        assert!(parse_claude_line(&value, Path::new("test.jsonl")).is_none());
    }

    #[test]
    fn claude_tool_use_records_pending_and_tool_result_clears() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/session-1.jsonl");

        let tool_use_json = r#"{
            "type": "assistant",
            "sessionId": "session-1",
            "message": {
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "call_abc",
                    "name": "Bash",
                    "input": { "command": "ls -la" }
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(tool_use_json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);
        assert!(tracker.pending.contains_key("call_abc"));
        assert_eq!(tracker.pending["call_abc"].source, SourceKind::ClaudeCode);

        let result_json = r#"{
            "type": "user",
            "sessionId": "session-1",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_abc",
                    "content": "file1.rs\nfile2.rs"
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(result_json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn claude_non_approval_tool_is_ignored() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/session-1.jsonl");

        let json = r#"{
            "type": "assistant",
            "sessionId": "session-1",
            "message": {
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "call_xyz",
                    "name": "Read",
                    "input": { "path": "/etc/hosts" }
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn claude_end_turn_is_not_tracked() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/session-1.jsonl");

        let json = r#"{
            "type": "assistant",
            "sessionId": "session-1",
            "message": {
                "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": "Done!" }]
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn claude_stale_tool_call_becomes_approval() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/sess.jsonl");

        let json = r#"{
            "type": "assistant",
            "sessionId": "sess",
            "message": {
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "call_stale",
                    "name": "Bash",
                    "input": { "command": "rm -rf /" }
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);

        let approvals = tracker.advance_poll();
        assert!(approvals.is_empty());

        let approvals = tracker.advance_poll();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].0, "sess");
        assert_eq!(approvals[0].1, SourceKind::ClaudeCode);
        assert_eq!(approvals[0].2, "Bash");
    }
}
