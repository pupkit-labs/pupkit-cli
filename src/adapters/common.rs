use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::protocol::{
    RequestId, SessionEvent, SessionEventKind, SessionEventPayload, SessionId, SourceKind,
};
use serde_json::Value;

pub fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToString::to_string)
}

pub fn infer_request_id(value: &Value, fallback_prefix: &str) -> RequestId {
    if let Some(request_id) = string_field(value, "request_id") {
        return RequestId::new(request_id);
    }

    let session_id =
        string_field(value, "session_id").unwrap_or_else(|| "unknown-session".to_string());
    let event_name = string_field(value, "hook_event_name")
        .or_else(|| string_field(value, "event"))
        .unwrap_or_else(|| "event".to_string());

    let fingerprint = format!(
        "{}|{}|{}|{}|{}|{}",
        fallback_prefix,
        session_id,
        event_name,
        value
            .get("question")
            .map(|v| v.to_string())
            .unwrap_or_default(),
        value
            .get("tool_name")
            .map(|v| v.to_string())
            .unwrap_or_default(),
        value
            .get("tool_input")
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
    let mut hasher = DefaultHasher::new();
    fingerprint.hash(&mut hasher);
    let hash = hasher.finish();

    RequestId::new(format!(
        "{fallback_prefix}-{session_id}-{event_name}-{hash:016x}"
    ))
}

pub fn summary_from_json(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".to_string())
}

pub fn started_event(source: SourceKind, session_id: String) -> SessionEvent {
    SessionEvent::new(
        source,
        SessionId::new(session_id),
        SessionEventKind::SessionStarted,
    )
}

pub fn approval_event(
    source: SourceKind,
    session_id: String,
    request_id: RequestId,
    tool_name: String,
    tool_input_summary: String,
) -> SessionEvent {
    SessionEvent::new(
        source,
        SessionId::new(session_id),
        SessionEventKind::ApprovalRequested,
    )
    .with_payload(SessionEventPayload::ApprovalRequest {
        request_id,
        tool_name,
        tool_input_summary,
    })
}

pub fn question_event(
    source: SourceKind,
    session_id: String,
    request_id: RequestId,
    prompt: String,
    options: Vec<String>,
) -> SessionEvent {
    SessionEvent::new(
        source,
        SessionId::new(session_id),
        SessionEventKind::QuestionRequested,
    )
    .with_payload(SessionEventPayload::QuestionRequest {
        request_id,
        prompt,
        options,
        allow_freeform: true,
    })
}

pub fn completion_event(
    source: SourceKind,
    session_id: String,
    headline: String,
    body: String,
) -> SessionEvent {
    SessionEvent::new(
        source,
        SessionId::new(session_id),
        SessionEventKind::CompletionPublished,
    )
    .with_payload(SessionEventPayload::Completion { headline, body })
}
