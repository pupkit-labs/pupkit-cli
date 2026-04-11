use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ApprovalBehavior {
    Allow,
    Deny,
    AllowAlways,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HookDecision {
    Ack,
    Approval { behavior: ApprovalBehavior },
    Cancelled,
    Timeout,
}
