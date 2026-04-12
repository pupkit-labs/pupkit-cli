use std::collections::BTreeMap;

use crate::protocol::{SessionId, SessionSnapshot, SessionStatus};

#[derive(Clone, Debug, Default)]
pub struct SessionRegistry {
    sessions: BTreeMap<SessionId, SessionSnapshot>,
}

/// How long (in seconds) before sessions in terminal states are removed.
const TTL_ENDED_SECS: u64 = 5 * 60; // 5 minutes
const TTL_COMPLETED_SECS: u64 = 30 * 60; // 30 minutes
/// Running sessions with no update for this long are marked Stale.
const STALE_THRESHOLD_SECS: u64 = 2 * 60 * 60; // 2 hours

impl SessionRegistry {
    pub fn upsert(&mut self, snapshot: SessionSnapshot) {
        self.sessions.insert(snapshot.session_id.clone(), snapshot);
    }

    pub fn get(&self, session_id: &SessionId) -> Option<&SessionSnapshot> {
        self.sessions.get(session_id)
    }

    pub fn get_mut(&mut self, session_id: &SessionId) -> Option<&mut SessionSnapshot> {
        self.sessions.get_mut(session_id)
    }

    pub fn all(&self) -> Vec<&SessionSnapshot> {
        self.sessions.values().collect()
    }

    pub fn replace_all(&mut self, sessions: Vec<SessionSnapshot>) {
        self.sessions = sessions
            .into_iter()
            .map(|snapshot| (snapshot.session_id.clone(), snapshot))
            .collect();
    }

    pub fn snapshots(&self) -> Vec<SessionSnapshot> {
        self.sessions.values().cloned().collect()
    }

    pub fn dismiss_attention_by_request(&mut self, request_id: &str) {
        for session in self.sessions.values_mut() {
            if session
                .attention
                .as_ref()
                .is_some_and(|a| a.request_id.as_str() == request_id)
            {
                session.attention = None;
                break;
            }
        }
    }

    pub fn clear_attentions(&mut self, source: Option<&str>) {
        for session in self.sessions.values_mut() {
            let matches = match source {
                None => true,
                Some(s) => format!("{:?}", session.source) == s,
            };
            if matches {
                session.attention = None;
            }
        }
    }

    /// Remove expired sessions and mark long-idle Running sessions as Stale.
    pub fn cleanup_expired(&mut self, now_secs: u64) {
        // First pass: mark long-idle Running sessions as Stale
        let stale_ids: Vec<SessionId> = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                s.status == SessionStatus::Running
                    && now_secs.saturating_sub(s.last_updated_at) >= STALE_THRESHOLD_SECS
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale_ids {
            if let Some(s) = self.sessions.get_mut(&id) {
                s.status = SessionStatus::Stale;
                s.attention = None;
                s.last_updated_at = now_secs; // TTL starts from when we mark it stale
            }
        }

        // Second pass: remove sessions past their TTL
        self.sessions.retain(|_, s| {
            let age = now_secs.saturating_sub(s.last_updated_at);
            match s.status {
                SessionStatus::Ended | SessionStatus::Stale => age < TTL_ENDED_SECS,
                SessionStatus::CompletedRecent | SessionStatus::Failed => {
                    age < TTL_COMPLETED_SECS
                }
                _ => true,
            }
        });
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

    fn make_session(id: &str, status: SessionStatus, updated_at: u64) -> SessionSnapshot {
        let mut s = SessionSnapshot::new(
            SessionId::new(id),
            SourceKind::ClaudeCode,
            id.to_string(),
            status,
        );
        s.last_updated_at = updated_at;
        s
    }

    #[test]
    fn cleanup_removes_ended_sessions_past_ttl() {
        let mut registry = SessionRegistry::default();
        let now = 10_000u64;
        registry.upsert(make_session("ended-old", SessionStatus::Ended, now - 400));
        registry.upsert(make_session("ended-new", SessionStatus::Ended, now - 100));
        registry.upsert(make_session("running", SessionStatus::Running, now - 100));

        registry.cleanup_expired(now);

        assert!(registry.get(&SessionId::new("ended-old")).is_none());
        assert!(registry.get(&SessionId::new("ended-new")).is_some());
        assert!(registry.get(&SessionId::new("running")).is_some());
    }

    #[test]
    fn cleanup_removes_stale_sessions_past_ttl() {
        let mut registry = SessionRegistry::default();
        let now = 10_000u64;
        registry.upsert(make_session("stale-old", SessionStatus::Stale, now - 400));
        registry.upsert(make_session("stale-new", SessionStatus::Stale, now - 60));

        registry.cleanup_expired(now);

        assert!(registry.get(&SessionId::new("stale-old")).is_none());
        assert!(registry.get(&SessionId::new("stale-new")).is_some());
    }

    #[test]
    fn cleanup_marks_idle_running_sessions_as_stale() {
        let mut registry = SessionRegistry::default();
        let now = 10_000u64;
        registry.upsert(make_session("idle", SessionStatus::Running, now - 7300));
        registry.upsert(make_session("active", SessionStatus::Running, now - 100));

        registry.cleanup_expired(now);

        assert_eq!(
            registry.get(&SessionId::new("idle")).unwrap().status,
            SessionStatus::Stale
        );
        assert_eq!(
            registry.get(&SessionId::new("active")).unwrap().status,
            SessionStatus::Running
        );
    }

    #[test]
    fn cleanup_removes_completed_sessions_past_ttl() {
        let mut registry = SessionRegistry::default();
        let now = 10_000u64;
        registry.upsert(make_session(
            "done-old",
            SessionStatus::CompletedRecent,
            now - 2000,
        ));
        registry.upsert(make_session(
            "done-new",
            SessionStatus::CompletedRecent,
            now - 100,
        ));

        registry.cleanup_expired(now);

        assert!(registry.get(&SessionId::new("done-old")).is_none());
        assert!(registry.get(&SessionId::new("done-new")).is_some());
    }
}
