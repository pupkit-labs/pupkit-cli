use std::collections::BTreeMap;

use crate::protocol::{AttentionKind, HookDecision, RequestId, SessionId, UserAnswer};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingRequest {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub kind: AttentionKind,
    pub created_at: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PendingStore {
    requests: BTreeMap<RequestId, PendingRequest>,
    request_order: Vec<RequestId>,
}

impl PendingStore {
    pub fn insert(&mut self, request: PendingRequest) {
        self.request_order.retain(|id| id != &request.request_id);
        self.request_order.push(request.request_id.clone());
        self.requests.insert(request.request_id.clone(), request);
    }

    pub fn remove(&mut self, request_id: &RequestId) -> Option<PendingRequest> {
        self.request_order.retain(|id| id != request_id);
        self.requests.remove(request_id)
    }

    pub fn clear_session(&mut self, session_id: &SessionId) -> Vec<PendingRequest> {
        let to_remove: Vec<RequestId> = self
            .requests
            .values()
            .filter(|request| &request.session_id == session_id)
            .map(|request| request.request_id.clone())
            .collect();
        to_remove
            .into_iter()
            .filter_map(|request_id| self.remove(&request_id))
            .collect()
    }

    pub fn resolve_approval(
        &mut self,
        request_id: &RequestId,
        behavior: crate::protocol::ApprovalBehavior,
    ) -> Option<HookDecision> {
        let request = self.remove(request_id)?;
        Some(HookDecision::Approval {
            request_id: request.request_id,
            behavior,
        })
    }

    pub fn resolve_answer(
        &mut self,
        request_id: &RequestId,
        answer: UserAnswer,
    ) -> Option<HookDecision> {
        let request = self.remove(request_id)?;
        Some(HookDecision::QuestionAnswer {
            request_id: request.request_id,
            answer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PendingRequest, PendingStore};
    use crate::protocol::{ApprovalBehavior, AttentionKind, RequestId, SessionId, UserAnswer};

    #[test]
    fn resolve_approval_removes_pending_request() {
        let mut store = PendingStore::default();
        let request_id = RequestId::new("req-1");
        store.insert(PendingRequest {
            request_id: request_id.clone(),
            session_id: SessionId::new("session-1"),
            kind: AttentionKind::Approval,
            created_at: 1,
        });

        let decision = store
            .resolve_approval(&request_id, ApprovalBehavior::Allow)
            .unwrap();

        assert!(matches!(
            decision,
            crate::protocol::HookDecision::Approval {
                behavior: ApprovalBehavior::Allow,
                ..
            }
        ));
        assert!(store.remove(&request_id).is_none());
    }

    #[test]
    fn resolve_answer_removes_pending_request() {
        let mut store = PendingStore::default();
        let request_id = RequestId::new("req-2");
        store.insert(PendingRequest {
            request_id: request_id.clone(),
            session_id: SessionId::new("session-2"),
            kind: AttentionKind::Question,
            created_at: 1,
        });

        let decision = store
            .resolve_answer(
                &request_id,
                UserAnswer::Option {
                    option_id: "yes".to_string(),
                },
            )
            .unwrap();

        assert!(matches!(
            decision,
            crate::protocol::HookDecision::QuestionAnswer { .. }
        ));
        assert!(store.remove(&request_id).is_none());
    }
}
