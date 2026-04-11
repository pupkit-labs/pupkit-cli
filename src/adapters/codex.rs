use serde_json::Value;

use crate::adapters::common::{
    approval_event, completion_event, infer_request_id, question_event, started_event,
    string_field, summary_from_json,
};
use crate::protocol::{SessionEvent, SourceKind};

pub fn normalize_codex_event(value: &Value) -> Result<SessionEvent, String> {
    let session_id = string_field(value, "session_id")
        .or_else(|| string_field(value, "sessionId"))
        .ok_or_else(|| "codex event missing session_id".to_string())?;
    let hook_event_name = string_field(value, "hook_event_name")
        .or_else(|| string_field(value, "event"))
        .unwrap_or_else(|| "session.started".to_string());

    let mut event = match hook_event_name.as_str() {
        "session.started" | "sessionStart" => started_event(SourceKind::Codex, session_id),
        "permission.request" | "PermissionRequest" => approval_event(
            SourceKind::Codex,
            session_id,
            infer_request_id(value, "codex"),
            string_field(value, "tool_name")
                .or_else(|| string_field(value, "toolName"))
                .unwrap_or_else(|| "Tool".to_string()),
            summary_from_json(value, "tool_input"),
        ),
        "question.request" | "QuestionRequest" => question_event(
            SourceKind::Codex,
            session_id,
            infer_request_id(value, "codex"),
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
        "task.completed" | "Stop" => completion_event(
            SourceKind::Codex,
            session_id,
            string_field(value, "summary").unwrap_or_else(|| "Task completed".to_string()),
            string_field(value, "reason").unwrap_or_default(),
        ),
        other => return Err(format!("unsupported codex event: {other}")),
    };

    if let Some(title) = string_field(value, "title") {
        event = event.with_title(title);
    }
    if let Some(cwd) = string_field(value, "cwd") {
        event = event.with_cwd(cwd);
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::normalize_codex_event;
    use crate::protocol::SessionEventKind;
    use serde_json::json;

    #[test]
    fn normalizes_codex_completion() {
        let value = json!({
            "session_id": "codex-session",
            "event": "task.completed",
            "summary": "done",
            "reason": "tests passed"
        });

        let event = normalize_codex_event(&value).unwrap();
        assert_eq!(event.kind, SessionEventKind::CompletionPublished);
    }
}
