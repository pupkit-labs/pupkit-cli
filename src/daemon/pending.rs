use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::protocol::{
    ApprovalBehavior, AttentionKind, HookDecision, RequestId, SessionId, UserAnswer,
};

#[derive(Clone, Debug)]
pub struct PendingWaitHandle {
    request_id: RequestId,
    inner: Arc<(Mutex<Option<HookDecision>>, Condvar)>,
}

impl PendingWaitHandle {
    fn new(request_id: RequestId) -> Self {
        Self {
            request_id,
            inner: Arc::new((Mutex::new(None), Condvar::new())),
        }
    }

    fn fulfill(&self, decision: HookDecision) {
        let (lock, condvar) = &*self.inner;
        let mut guard = lock.lock().unwrap();
        *guard = Some(decision);
        condvar.notify_all();
    }

    pub fn wait_for_decision(&self, timeout: Duration) -> HookDecision {
        let (lock, condvar) = &*self.inner;
        let guard = lock.lock().unwrap();
        let (mut guard, timeout_result) = condvar
            .wait_timeout_while(guard, timeout, |decision| decision.is_none())
            .unwrap();

        if let Some(decision) = guard.take() {
            decision
        } else if timeout_result.timed_out() {
            HookDecision::Timeout {
                request_id: self.request_id.clone(),
            }
        } else {
            HookDecision::Cancelled {
                request_id: self.request_id.clone(),
            }
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PendingRequest {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub kind: AttentionKind,
    pub created_at: u64,
    wait_handle: PendingWaitHandle,
}

impl PendingRequest {
    pub fn new(
        request_id: RequestId,
        session_id: SessionId,
        kind: AttentionKind,
        created_at: u64,
    ) -> (Self, PendingWaitHandle) {
        let wait_handle = PendingWaitHandle::new(request_id.clone());
        (
            Self {
                request_id,
                session_id,
                kind,
                created_at,
                wait_handle: wait_handle.clone(),
            },
            wait_handle,
        )
    }

    fn fulfill(&self, decision: HookDecision) {
        self.wait_handle.fulfill(decision);
    }
}

#[derive(Debug, Default)]
pub struct PendingStore {
    requests: BTreeMap<RequestId, PendingRequest>,
    request_order: Vec<RequestId>,
}

impl PendingStore {
    pub fn insert(&mut self, request: PendingRequest) {
        if let Some(previous) = self.requests.remove(&request.request_id) {
            previous.fulfill(HookDecision::Cancelled {
                request_id: previous.request_id.clone(),
            });
        }
        self.request_order.retain(|id| id != &request.request_id);
        self.request_order.push(request.request_id.clone());
        self.requests.insert(request.request_id.clone(), request);
    }

    pub fn remove(&mut self, request_id: &RequestId) -> Option<PendingRequest> {
        self.request_order.retain(|id| id != request_id);
        self.requests.remove(request_id)
    }

    pub fn cancel_session(&mut self, session_id: &SessionId) {
        let to_remove: Vec<RequestId> = self
            .requests
            .values()
            .filter(|request| &request.session_id == session_id)
            .map(|request| request.request_id.clone())
            .collect();
        for request_id in to_remove {
            if let Some(request) = self.remove(&request_id) {
                request.fulfill(HookDecision::Cancelled {
                    request_id: request.request_id.clone(),
                });
            }
        }
    }

    pub fn abandon_request(&mut self, request_id: &RequestId) {
        let _ = self.remove(request_id);
    }

    pub fn resolve_approval(
        &mut self,
        request_id: &RequestId,
        behavior: ApprovalBehavior,
    ) -> Option<HookDecision> {
        let request = self.remove(request_id)?;
        let decision = HookDecision::Approval {
            request_id: request.request_id.clone(),
            behavior,
        };
        request.fulfill(decision.clone());
        Some(decision)
    }

    pub fn resolve_answer(
        &mut self,
        request_id: &RequestId,
        answer: UserAnswer,
    ) -> Option<HookDecision> {
        let request = self.remove(request_id)?;
        let decision = HookDecision::QuestionAnswer {
            request_id: request.request_id.clone(),
            answer,
        };
        request.fulfill(decision.clone());
        Some(decision)
    }
}

#[cfg(test)]
mod tests {
    use super::{PendingRequest, PendingStore};
    use crate::protocol::{ApprovalBehavior, AttentionKind, RequestId, SessionId, UserAnswer};
    use std::time::Duration;

    #[test]
    fn resolve_approval_removes_pending_request() {
        let mut store = PendingStore::default();
        let request_id = RequestId::new("req-1");
        let (request, waiter) = PendingRequest::new(
            request_id.clone(),
            SessionId::new("session-1"),
            AttentionKind::Approval,
            1,
        );
        store.insert(request);

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
        assert!(matches!(
            waiter.wait_for_decision(Duration::from_millis(1)),
            crate::protocol::HookDecision::Approval { .. }
        ));
    }

    #[test]
    fn resolve_answer_removes_pending_request() {
        let mut store = PendingStore::default();
        let request_id = RequestId::new("req-2");
        let (request, waiter) = PendingRequest::new(
            request_id.clone(),
            SessionId::new("session-2"),
            AttentionKind::Question,
            1,
        );
        store.insert(request);

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
        assert!(matches!(
            waiter.wait_for_decision(Duration::from_millis(1)),
            crate::protocol::HookDecision::QuestionAnswer { .. }
        ));
    }
}
