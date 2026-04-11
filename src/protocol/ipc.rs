use serde::{Deserialize, Serialize};

use crate::protocol::{RequestId, SessionEvent, SessionId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HookEnvelope {
    pub event: SessionEvent,
    pub expects_response: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UiAction {
    Approve { request_id: RequestId, always: bool },
    Deny { request_id: RequestId },
    DismissCompletion { session_id: SessionId },
}
