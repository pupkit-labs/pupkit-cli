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

/// Collect all sessions that have an active attention, sorted by priority.
pub fn collect_attention_sessions<'a>(
    sessions: impl IntoIterator<Item = &'a SessionSnapshot>,
) -> Vec<&'a SessionSnapshot> {
    let mut with_attention: Vec<&SessionSnapshot> = sessions
        .into_iter()
        .filter(|s| s.attention.is_some())
        .collect();
    with_attention.sort_by_key(|s| {
        (
            s.status.priority_rank(),
            std::cmp::Reverse(s.last_updated_at),
            s.title.as_str(),
        )
    });
    with_attention
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

    #[test]
    fn collect_attention_returns_only_sessions_with_attention() {
        use super::collect_attention_sessions;
        use crate::protocol::{AttentionSnapshot, AttentionKind, RequestId};

        let running = SessionSnapshot::new(
            SessionId::new("no-attention"),
            SourceKind::ClaudeCode,
            "running".to_string(),
            SessionStatus::Running,
        );
        let mut with_attention = SessionSnapshot::new(
            SessionId::new("has-attention"),
            SourceKind::Copilot,
            "waiting".to_string(),
            SessionStatus::WaitingApproval,
        );
        with_attention.attention = Some(AttentionSnapshot {
            kind: AttentionKind::Approval,
            request_id: RequestId::new("req-1"),
            message: "approve?".to_string(),
            options: vec![],
            allow_freeform: false,
        });

        let result = collect_attention_sessions([&running, &with_attention]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id.as_str(), "has-attention");
    }
}
