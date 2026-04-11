use serde::{Deserialize, Serialize};

use crate::protocol::SessionId;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SourceKind {
    ClaudeCode,
    Codex,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionStatus {
    Running,
    WaitingApproval,
    WaitingQuestion,
    CompletedRecent,
    Failed,
    Ended,
    Stale,
}

impl SessionStatus {
    pub fn requires_attention(&self) -> bool {
        matches!(
            self,
            Self::WaitingApproval | Self::WaitingQuestion | Self::Failed
        )
    }

    pub fn priority_rank(&self) -> u8 {
        match self {
            Self::WaitingApproval => 0,
            Self::WaitingQuestion => 1,
            Self::Failed => 2,
            Self::CompletedRecent => 3,
            Self::Running => 4,
            Self::Stale => 5,
            Self::Ended => 6,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub source: SourceKind,
    pub title: String,
    pub status: SessionStatus,
}

impl SessionSnapshot {
    pub fn new(
        session_id: SessionId,
        source: SourceKind,
        title: String,
        status: SessionStatus,
    ) -> Self {
        Self {
            session_id,
            source,
            title,
            status,
        }
    }
}
