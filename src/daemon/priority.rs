use crate::protocol::SessionSnapshot;

pub fn select_top_session<'a>(
    sessions: impl IntoIterator<Item = &'a SessionSnapshot>,
) -> Option<&'a SessionSnapshot> {
    sessions.into_iter().min_by_key(|snapshot| {
        (
            snapshot.status.priority_rank(),
            std::cmp::Reverse(snapshot.last_updated_at),
            snapshot.title.as_str(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::select_top_session;
    use crate::protocol::{SessionId, SessionSnapshot, SessionStatus, SourceKind};

    #[test]
    fn waiting_approval_ranks_ahead_of_running_sessions() {
        let mut running = SessionSnapshot::new(
            SessionId::new("running"),
            SourceKind::ClaudeCode,
            "running".to_string(),
            SessionStatus::Running,
        );
        running.last_updated_at = 1;
        let mut waiting = SessionSnapshot::new(
            SessionId::new("waiting"),
            SourceKind::Codex,
            "waiting".to_string(),
            SessionStatus::WaitingApproval,
        );
        waiting.last_updated_at = 2;

        let selected = select_top_session([&running, &waiting]).unwrap();
        assert_eq!(selected.session_id.as_str(), "waiting");
    }
}
