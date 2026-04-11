use serde::{Deserialize, Serialize};

use crate::protocol::{SessionId, SourceKind};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SessionEventKind {
    SessionStarted,
    SessionUpdated,
    AttentionRequired,
    CompletionPublished,
    FailurePublished,
    SessionEnded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    pub source: SourceKind,
    pub session_id: SessionId,
    pub kind: SessionEventKind,
}

impl SessionEvent {
    pub fn new(source: SourceKind, session_id: SessionId, kind: SessionEventKind) -> Self {
        Self {
            source,
            session_id,
            kind,
        }
    }
}
