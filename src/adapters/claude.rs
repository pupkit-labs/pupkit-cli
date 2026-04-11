use serde_json::Value;

use crate::adapters::common::{
    approval_event, completion_event, infer_request_id, question_event, started_event,
    string_field, summary_from_json,
};
use crate::protocol::{SessionEvent, SourceKind};

pub fn normalize_claude_event(value: &Value) -> Result<SessionEvent, String> {
    let session_id = string_field(value, "session_id")
        .ok_or_else(|| "claude event missing session_id".to_string())?;
    let hook_event_name =
        string_field(value, "hook_event_name").unwrap_or_else(|| "sessionStart".to_string());

    let mut event = match hook_event_name.as_str() {
        "sessionStart" | "sessionStarted" => started_event(SourceKind::ClaudeCode, session_id),
        "PermissionRequest" => approval_event(
            SourceKind::ClaudeCode,
            session_id,
            infer_request_id(value, "claude"),
            string_field(value, "tool_name").unwrap_or_else(|| "Tool".to_string()),
            summary_from_json(value, "tool_input"),
        ),
        "Notification" | "question" => question_event(
            SourceKind::ClaudeCode,
            session_id,
            infer_request_id(value, "claude"),
            string_field(value, "question").unwrap_or_else(|| "Question".to_string()),
            value
                .get("options")
                .and_then(|value| value.as_array())
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|option| option.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        ),
        "Stop" | "SubagentStop" => completion_event(
            SourceKind::ClaudeCode,
            session_id,
            string_field(value, "summary").unwrap_or_else(|| "Task completed".to_string()),
            string_field(value, "reason").unwrap_or_default(),
        ),
        other => {
            return Err(format!("unsupported claude hook event: {other}"));
        }
    };

    if let Some(title) = string_field(value, "title") {
        event = event.with_title(title);
    }
    if let Some(cwd) = string_field(value, "cwd") {
        event = event.with_cwd(cwd);
    }
    if let Some(summary) = string_field(value, "summary") {
        event = event.with_summary(summary);
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::normalize_claude_event;
    use crate::protocol::SessionEventKind;
    use serde_json::json;

    #[test]
    fn normalizes_permission_request() {
        let value = json!({
            "session_id": "claude-session",
            "hook_event_name": "PermissionRequest",
            "tool_name": "Edit",
            "tool_input": {"path": "src/lib.rs"},
            "title": "fix bug"
        });

        let event = normalize_claude_event(&value).unwrap();
        assert_eq!(event.kind, SessionEventKind::ApprovalRequested);
        assert_eq!(event.title.as_deref(), Some("fix bug"));
    }
}
