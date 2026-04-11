use serde::{Deserialize, Serialize};

use crate::protocol::{
    HookDecision, RequestId, SessionEvent, SessionId, SessionStatus, SourceKind,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HookEnvelope {
    pub event: SessionEvent,
    pub expects_response: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AttentionCard {
    pub session_id: SessionId,
    pub request_id: RequestId,
    pub source: SourceKind,
    pub title: String,
    pub status: SessionStatus,
    pub message: String,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionListItem {
    pub session_id: SessionId,
    pub source: SourceKind,
    pub title: String,
    pub status: SessionStatus,
    pub summary: Option<String>,
    pub last_updated_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompletionItem {
    pub session_id: SessionId,
    pub source: SourceKind,
    pub title: String,
    pub headline: String,
    pub body: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UiStateSnapshot {
    pub top_attention: Option<AttentionCard>,
    pub sessions: Vec<SessionListItem>,
    pub recent_completions: Vec<CompletionItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UiAction {
    Approve {
        request_id: RequestId,
        always: bool,
    },
    Deny {
        request_id: RequestId,
    },
    AnswerOption {
        request_id: RequestId,
        option_id: String,
    },
    AnswerText {
        request_id: RequestId,
        text: String,
    },
    DismissCompletion {
        session_id: SessionId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ClientRequest {
    Hook(HookEnvelope),
    Ui(UiAction),
    StateSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ServerResponse {
    Ack,
    HookDecision(HookDecision),
    UiActionResult {
        decision: Option<HookDecision>,
        state: UiStateSnapshot,
    },
    StateSnapshot(UiStateSnapshot),
    Error {
        message: String,
    },
}
