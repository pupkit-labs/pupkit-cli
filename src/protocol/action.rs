use serde::{Deserialize, Serialize};

use crate::protocol::RequestId;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ApprovalBehavior {
    Allow,
    Deny,
    AllowAlways,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UserAnswer {
    Option { option_id: String },
    Text { value: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HookDecision {
    Ack,
    Approval {
        request_id: RequestId,
        behavior: ApprovalBehavior,
    },
    QuestionAnswer {
        request_id: RequestId,
        answer: UserAnswer,
    },
    Cancelled {
        request_id: RequestId,
    },
    Timeout {
        request_id: RequestId,
    },
}
