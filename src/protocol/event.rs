use serde::{Deserialize, Serialize};

use crate::protocol::{RequestId, SessionId, SourceKind};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionEventKind {
    SessionStarted,
    SessionUpdated,
    ApprovalRequested,
    QuestionRequested,
    CompletionPublished,
    FailurePublished,
    SessionEnded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionEventPayload {
    None,
    Summary {
        message: String,
    },
    ApprovalRequest {
        request_id: RequestId,
        tool_name: String,
        tool_input_summary: String,
    },
    QuestionRequest {
        request_id: RequestId,
        prompt: String,
        options: Vec<String>,
    },
    Completion {
        headline: String,
        body: String,
    },
    Failure {
        headline: String,
        body: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub source: SourceKind,
    pub session_id: SessionId,
    pub kind: SessionEventKind,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub summary: Option<String>,
    pub occurred_at: u64,
    pub payload: SessionEventPayload,
}

impl SessionEvent {
    pub fn new(source: SourceKind, session_id: SessionId, kind: SessionEventKind) -> Self {
        Self {
            source,
            session_id,
            kind,
            title: None,
            cwd: None,
            summary: None,
            occurred_at: 0,
            payload: SessionEventPayload::None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn with_payload(mut self, payload: SessionEventPayload) -> Self {
        self.payload = payload;
        self
    }

    pub fn with_occurred_at(mut self, occurred_at: u64) -> Self {
        self.occurred_at = occurred_at;
        self
    }
}
