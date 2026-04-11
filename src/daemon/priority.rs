use crate::protocol::SessionSnapshot;

pub fn select_top_session<'a>(
    sessions: impl IntoIterator<Item = &'a SessionSnapshot>,
) -> Option<&'a SessionSnapshot> {
    sessions
        .into_iter()
        .min_by_key(|snapshot| (snapshot.status.priority_rank(), snapshot.title.as_str()))
}

#[cfg(test)]
mod tests {
    use super::select_top_session;
    use crate::protocol::{SessionId, SessionSnapshot, SessionStatus, SourceKind};

    #[test]
    fn waiting_approval_ranks_ahead_of_running_sessions() {
        let running = SessionSnapshot::new(
            SessionId::new("running"),
            SourceKind::ClaudeCode,
            "running".to_string(),
            SessionStatus::Running,
        );
        let waiting = SessionSnapshot::new(
            SessionId::new("waiting"),
            SourceKind::Codex,
            "waiting".to_string(),
            SessionStatus::WaitingApproval,
        );

        let selected = select_top_session([&running, &waiting]).unwrap();
        assert_eq!(selected.session_id.as_str(), "waiting");
    }
}
