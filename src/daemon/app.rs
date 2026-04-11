use std::env;

use crate::daemon::persistence::{PersistentDaemonState, load_state, save_state};
use crate::daemon::{DaemonConfig, SessionRegistry, select_top_session};
use crate::protocol::{
    ApprovalBehavior, AttentionCard, AttentionKind, AttentionSnapshot, CompletionItem,
    HookDecision, SessionEvent, SessionEventKind, SessionEventPayload, SessionListItem,
    SessionSnapshot, SessionStatus, UiAction, UiStateSnapshot, UserAnswer,
};

use super::pending::{PendingRequest, PendingStore};

#[derive(Clone, Debug)]
pub struct PupkitDaemon {
    config: DaemonConfig,
    registry: SessionRegistry,
    pending: PendingStore,
    completions: Vec<CompletionItem>,
}

impl PupkitDaemon {
    pub fn bootstrap() -> Self {
        let home = env::var_os("HOME").map(Into::into);
        let config = DaemonConfig::default_for_home(home);
        let mut daemon = Self {
            config,
            registry: SessionRegistry::default(),
            pending: PendingStore::default(),
            completions: Vec::new(),
        };
        let _ = daemon.restore_state();
        daemon
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn ingest_event(&mut self, event: SessionEvent) -> Result<(), String> {
        let mut snapshot = self
            .registry
            .get(&event.session_id)
            .cloned()
            .unwrap_or_else(|| {
                SessionSnapshot::new(
                    event.session_id.clone(),
                    event.source.clone(),
                    event
                        .title
                        .clone()
                        .unwrap_or_else(|| event.session_id.as_str().to_string()),
                    SessionStatus::Running,
                )
            });

        if let Some(title) = event.title.clone() {
            snapshot.title = title;
        }
        if let Some(cwd) = event.cwd.clone() {
            snapshot.cwd = Some(cwd);
        }
        snapshot.last_updated_at = event.occurred_at;
        if let Some(summary) = event.summary.clone() {
            snapshot.last_summary = Some(summary);
        }

        match event.kind {
            SessionEventKind::SessionStarted | SessionEventKind::SessionUpdated => {
                snapshot.status = SessionStatus::Running;
                snapshot.attention = None;
            }
            SessionEventKind::ApprovalRequested => {
                if let SessionEventPayload::ApprovalRequest {
                    request_id,
                    tool_name,
                    tool_input_summary,
                } = event.payload
                {
                    self.pending.clear_session(&event.session_id);
                    self.pending.insert(PendingRequest {
                        request_id: request_id.clone(),
                        session_id: event.session_id.clone(),
                        kind: AttentionKind::Approval,
                        created_at: event.occurred_at,
                    });
                    snapshot.status = SessionStatus::WaitingApproval;
                    snapshot.attention = Some(AttentionSnapshot {
                        request_id,
                        kind: AttentionKind::Approval,
                        message: format!("{tool_name}: {tool_input_summary}"),
                        options: vec!["allow".to_string(), "deny".to_string()],
                    });
                }
            }
            SessionEventKind::QuestionRequested => {
                if let SessionEventPayload::QuestionRequest {
                    request_id,
                    prompt,
                    options,
                } = event.payload
                {
                    self.pending.clear_session(&event.session_id);
                    self.pending.insert(PendingRequest {
                        request_id: request_id.clone(),
                        session_id: event.session_id.clone(),
                        kind: AttentionKind::Question,
                        created_at: event.occurred_at,
                    });
                    snapshot.status = SessionStatus::WaitingQuestion;
                    snapshot.attention = Some(AttentionSnapshot {
                        request_id,
                        kind: AttentionKind::Question,
                        message: prompt,
                        options,
                    });
                }
            }
            SessionEventKind::CompletionPublished => {
                if let SessionEventPayload::Completion { headline, body } = event.payload {
                    self.pending.clear_session(&event.session_id);
                    snapshot.status = SessionStatus::CompletedRecent;
                    snapshot.attention = None;
                    snapshot.last_summary = Some(headline.clone());
                    self.completions.insert(
                        0,
                        CompletionItem {
                            session_id: event.session_id.clone(),
                            source: event.source.clone(),
                            title: snapshot.title.clone(),
                            headline,
                            body,
                        },
                    );
                    self.completions.truncate(10);
                }
            }
            SessionEventKind::FailurePublished => {
                if let SessionEventPayload::Failure { headline, body } = event.payload {
                    self.pending.clear_session(&event.session_id);
                    snapshot.status = SessionStatus::Failed;
                    snapshot.attention = None;
                    snapshot.last_summary = Some(format!("{headline}: {body}"));
                }
            }
            SessionEventKind::SessionEnded => {
                self.pending.clear_session(&event.session_id);
                snapshot.status = SessionStatus::Ended;
                snapshot.attention = None;
            }
        }

        self.registry.upsert(snapshot);
        self.persist_state()?;
        Ok(())
    }

    pub fn apply_ui_action(&mut self, action: UiAction) -> Result<Option<HookDecision>, String> {
        let decision = match action {
            UiAction::Approve { request_id, always } => self.pending.resolve_approval(
                &request_id,
                if always {
                    ApprovalBehavior::AllowAlways
                } else {
                    ApprovalBehavior::Allow
                },
            ),
            UiAction::Deny { request_id } => self
                .pending
                .resolve_approval(&request_id, ApprovalBehavior::Deny),
            UiAction::AnswerOption {
                request_id,
                option_id,
            } => self
                .pending
                .resolve_answer(&request_id, UserAnswer::Option { option_id }),
            UiAction::AnswerText { request_id, text } => self
                .pending
                .resolve_answer(&request_id, UserAnswer::Text { value: text }),
            UiAction::DismissCompletion { session_id } => {
                self.completions
                    .retain(|item| item.session_id != session_id);
                None
            }
        };

        let resolved_request_id = decision.as_ref().and_then(|decision| match decision {
            HookDecision::Approval { request_id, .. }
            | HookDecision::QuestionAnswer { request_id, .. }
            | HookDecision::Cancelled { request_id }
            | HookDecision::Timeout { request_id } => Some(request_id.clone()),
            HookDecision::Ack => None,
        });

        if let Some(request_id) = resolved_request_id {
            for snapshot in self.registry.all() {
                if snapshot
                    .attention
                    .as_ref()
                    .is_some_and(|attention| attention.request_id == request_id)
                {
                    let mut updated = snapshot.clone();
                    updated.status = SessionStatus::Running;
                    updated.attention = None;
                    self.registry.upsert(updated);
                    break;
                }
            }
        }

        self.persist_state()?;
        Ok(decision)
    }

    pub fn state_snapshot(&self) -> UiStateSnapshot {
        let sessions: Vec<SessionListItem> = self
            .registry
            .all()
            .into_iter()
            .map(|snapshot| SessionListItem {
                session_id: snapshot.session_id.clone(),
                source: snapshot.source.clone(),
                title: snapshot.title.clone(),
                status: snapshot.status.clone(),
                summary: snapshot.last_summary.clone(),
            })
            .collect();

        let top_attention = select_top_session(self.registry.all()).and_then(|snapshot| {
            let attention = snapshot.attention.as_ref()?;
            Some(AttentionCard {
                session_id: snapshot.session_id.clone(),
                request_id: attention.request_id.clone(),
                source: snapshot.source.clone(),
                title: snapshot.title.clone(),
                status: snapshot.status.clone(),
                message: attention.message.clone(),
                options: attention.options.clone(),
            })
        });

        UiStateSnapshot {
            top_attention,
            sessions,
            recent_completions: self.completions.clone(),
        }
    }

    pub fn report(&self) -> String {
        let state = self.state_snapshot();
        let top = state
            .top_attention
            .as_ref()
            .map(|card| format!("{} [{}]", card.title, card.message))
            .unwrap_or_else(|| "none".to_string());
        format!(
            "pupkit daemon ready
socket: {}
state: {}
sessions: {}
top attention: {}",
            self.config.socket_path.display(),
            self.config.state_path.display(),
            state.sessions.len(),
            top,
        )
    }

    pub fn persist_state(&self) -> Result<(), String> {
        let state = PersistentDaemonState {
            sessions: self.registry.snapshots(),
            recent_completions: self.completions.clone(),
        };
        save_state(&self.config.state_path, &state)
    }

    fn restore_state(&mut self) -> Result<(), String> {
        let state = load_state(&self.config.state_path)?;
        self.registry.replace_all(state.sessions);
        self.completions = state.recent_completions;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::PupkitDaemon;
    use crate::protocol::{
        RequestId, SessionEvent, SessionEventKind, SessionEventPayload, SessionId, SourceKind,
        UiAction,
    };

    #[test]
    fn top_attention_prefers_approval_requests() {
        let mut daemon = PupkitDaemon::bootstrap();
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::ClaudeCode,
                    SessionId::new("session-running"),
                    SessionEventKind::SessionStarted,
                )
                .with_title("running")
                .with_occurred_at(1),
            )
            .unwrap();
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::Codex,
                    SessionId::new("session-approval"),
                    SessionEventKind::ApprovalRequested,
                )
                .with_title("approval")
                .with_payload(SessionEventPayload::ApprovalRequest {
                    request_id: RequestId::new("req-1"),
                    tool_name: "Edit".to_string(),
                    tool_input_summary: "modify src/main.rs".to_string(),
                })
                .with_occurred_at(2),
            )
            .unwrap();

        let snapshot = daemon.state_snapshot();
        assert_eq!(
            snapshot.top_attention.unwrap().session_id.as_str(),
            "session-approval"
        );
    }

    #[test]
    fn approve_action_clears_attention() {
        let mut daemon = PupkitDaemon::bootstrap();
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::ClaudeCode,
                    SessionId::new("session-approval"),
                    SessionEventKind::ApprovalRequested,
                )
                .with_title("approval")
                .with_payload(SessionEventPayload::ApprovalRequest {
                    request_id: RequestId::new("req-1"),
                    tool_name: "Edit".to_string(),
                    tool_input_summary: "modify src/main.rs".to_string(),
                })
                .with_occurred_at(2),
            )
            .unwrap();

        let decision = daemon
            .apply_ui_action(UiAction::Approve {
                request_id: RequestId::new("req-1"),
                always: false,
            })
            .unwrap();
        assert!(decision.is_some());
        assert!(daemon.state_snapshot().top_attention.is_none());
    }
}
