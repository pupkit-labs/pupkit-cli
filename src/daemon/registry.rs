use std::collections::BTreeMap;

use crate::protocol::{SessionId, SessionSnapshot};

#[derive(Clone, Debug, Default)]
pub struct SessionRegistry {
    sessions: BTreeMap<SessionId, SessionSnapshot>,
}

impl SessionRegistry {
    pub fn upsert(&mut self, snapshot: SessionSnapshot) {
        self.sessions.insert(snapshot.session_id.clone(), snapshot);
    }

    pub fn get(&self, session_id: &SessionId) -> Option<&SessionSnapshot> {
        self.sessions.get(session_id)
    }

    pub fn all(&self) -> Vec<&SessionSnapshot> {
        self.sessions.values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::SessionRegistry;
    use crate::protocol::{SessionId, SessionSnapshot, SessionStatus, SourceKind};

    #[test]
    fn upsert_replaces_existing_session_snapshot() {
        let mut registry = SessionRegistry::default();
        let session_id = SessionId::new("session-1");
        registry.upsert(SessionSnapshot::new(
            session_id.clone(),
            SourceKind::ClaudeCode,
            "first".to_string(),
            SessionStatus::Running,
        ));
        registry.upsert(SessionSnapshot::new(
            session_id.clone(),
            SourceKind::ClaudeCode,
            "updated".to_string(),
            SessionStatus::WaitingApproval,
        ));

        let snapshot = registry.get(&session_id).unwrap();
        assert_eq!(snapshot.title, "updated");
        assert_eq!(snapshot.status, SessionStatus::WaitingApproval);
    }
}
