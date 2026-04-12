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
    pub allow_freeform: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionListItem {
    pub session_id: SessionId,
    pub source: SourceKind,
    pub title: String,
    pub status: SessionStatus,
    pub summary: Option<String>,
    pub cwd: Option<String>,
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

/// Compact usage metrics for the Dynamic Island notch header.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsageCompact {
    /// Claude Code 24h total tokens (raw count, UI formats as K/M)
    pub claude_24h_tokens: Option<u64>,
    /// Claude Code 7d total tokens
    pub claude_7d_tokens: Option<u64>,
    /// Codex primary (5h) remaining percentage (0-100)
    pub codex_5h_remaining_pct: Option<u8>,
    /// Codex secondary (7d) remaining percentage (0-100)
    pub codex_7d_remaining_pct: Option<u8>,
    /// Copilot Premium requests remaining percentage × 10 (e.g. 956 = 95.6%)
    pub copilot_premium_remaining_pct_x10: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UiStateSnapshot {
    pub attentions: Vec<AttentionCard>,
    pub sessions: Vec<SessionListItem>,
    pub recent_completions: Vec<CompletionItem>,
    #[serde(default)]
    pub usage: Option<UsageCompact>,
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
    DismissAttention {
        request_id: RequestId,
    },
    ClearAttentions {
        source: Option<String>,
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
