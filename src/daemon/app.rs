use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{log_debug, log_error, log_warn};

use crate::daemon::pending::{PendingRequest, PendingStore, PendingWaitHandle};
use crate::daemon::persistence::{PersistentDaemonState, load_state, save_state};
use crate::daemon::tty_inject::CopilotTtyStore;
use crate::daemon::{DaemonConfig, SessionRegistry, collect_attention_sessions, select_top_session};
use crate::protocol::{
    ApprovalBehavior, AttentionCard, AttentionKind, AttentionSnapshot, CompletionItem,
    HookDecision, RequestId, SessionEvent, SessionEventKind, SessionEventPayload, SessionListItem,
    SessionSnapshot, SessionStatus, SourceKind, UiAction, UiStateSnapshot, UsageCompact, UserAnswer,
};

#[derive(Debug)]
pub struct PupkitDaemon {
    config: DaemonConfig,
    registry: SessionRegistry,
    pending: PendingStore,
    completions: Vec<CompletionItem>,
    copilot_ttys: CopilotTtyStore,
    usage: Option<UsageCompact>,
}

impl PupkitDaemon {
    pub fn bootstrap() -> Self {
        let home = env::var_os("HOME").map(Into::into);
        let config = DaemonConfig::default_for_home(home);
        Self::for_config(config)
    }

    pub fn for_config(config: DaemonConfig) -> Self {
        let mut daemon = Self {
            config,
            registry: SessionRegistry::default(),
            pending: PendingStore::default(),
            completions: Vec::new(),
            copilot_ttys: CopilotTtyStore::default(),
            usage: None,
        };
        if let Err(error) = daemon.restore_state() {
            log_warn!("{error}");
        }
        daemon
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn copilot_ttys_mut(&mut self) -> &mut CopilotTtyStore {
        &mut self.copilot_ttys
    }

    pub fn update_usage(&mut self, usage: UsageCompact) {
        self.usage = Some(usage);
    }

    pub fn ingest_event(&mut self, event: SessionEvent) -> Result<(), String> {
        let _ = self.ingest_event_internal(event, false)?;
        Ok(())
    }

    pub fn ingest_blocking_event(
        &mut self,
        event: SessionEvent,
    ) -> Result<PendingWaitHandle, String> {
        self.ingest_event_internal(event, true)?
            .ok_or_else(|| "blocking event did not produce a pending wait handle".to_string())
    }

    fn ingest_event_internal(
        &mut self,
        event: SessionEvent,
        expect_blocking_waiter: bool,
    ) -> Result<Option<PendingWaitHandle>, String> {
        let mut snapshot = self
            .registry
            .get(&event.session_id)
            .cloned()
            .unwrap_or_else(|| {
                let default_title = match event.source {
                    SourceKind::Copilot => "Copilot Chat".to_string(),
                    SourceKind::ClaudeCode => "Claude Code".to_string(),
                    SourceKind::Codex => "Codex".to_string(),
                    _ => event.session_id.as_str().to_string(),
                };
                SessionSnapshot::new(
                    event.session_id.clone(),
                    event.source.clone(),
                    event.title.clone().unwrap_or(default_title),
                    SessionStatus::Running,
                )
            });

        if let Some(title) = event.title.clone() {
            snapshot.title = title;
        }
        if let Some(cwd) = event.cwd.clone() {
            snapshot.cwd = Some(cwd.clone());
            // Auto-derive project-aware title from CWD when title lacks folder context
            if let Some(folder) = cwd.rsplit('/').next().filter(|s| !s.is_empty()) {
                let prefix = match event.source {
                    SourceKind::Copilot => "Copilot",
                    SourceKind::ClaudeCode => "Claude Code",
                    SourceKind::Codex => "Codex",
                    _ => "",
                };
                if !prefix.is_empty() && !snapshot.title.contains('·') {
                    snapshot.title = format!("{prefix} · {folder}");
                }
            }
        }
        snapshot.last_updated_at = if event.occurred_at == 0 {
            current_unix_timestamp()
        } else {
            event.occurred_at
        };
        if let Some(summary) = event.summary.clone() {
            snapshot.last_summary = Some(summary);
        }

        let mut wait_handle = None;

        match event.kind {
            SessionEventKind::SessionStarted | SessionEventKind::SessionUpdated => {
                // Don't clear attention if we're waiting for user input
                if !matches!(
                    snapshot.status,
                    SessionStatus::WaitingApproval | SessionStatus::WaitingQuestion
                ) {
                    snapshot.status = SessionStatus::Running;
                    snapshot.attention = None;
                }
            }
            SessionEventKind::ApprovalRequested => {
                if let SessionEventPayload::ApprovalRequest {
                    request_id,
                    tool_name,
                    tool_input_summary,
                } = event.payload
                {
                    self.pending.cancel_session(&event.session_id);
                    let (request, waiter) = PendingRequest::new(
                        request_id.clone(),
                        event.session_id.clone(),
                        AttentionKind::Approval,
                        snapshot.last_updated_at,
                    );
                    self.pending.insert(request);
                    snapshot.status = SessionStatus::WaitingApproval;
                    snapshot.attention = Some(AttentionSnapshot {
                        request_id,
                        kind: AttentionKind::Approval,
                        message: format!("{tool_name}: {tool_input_summary}"),
                        options: vec!["allow".to_string(), "deny".to_string()],
                        allow_freeform: false,
                    });
                    wait_handle = Some(waiter);
                }
            }
            SessionEventKind::QuestionRequested => {
                if let SessionEventPayload::QuestionRequest {
                    request_id,
                    prompt,
                    options,
                    allow_freeform,
                } = event.payload
                {
                    self.pending.cancel_session(&event.session_id);
                    let (request, waiter) = PendingRequest::new(
                        request_id.clone(),
                        event.session_id.clone(),
                        AttentionKind::Question,
                        snapshot.last_updated_at,
                    );
                    self.pending.insert(request);
                    snapshot.status = SessionStatus::WaitingQuestion;
                    snapshot.attention = Some(AttentionSnapshot {
                        request_id,
                        kind: AttentionKind::Question,
                        message: prompt,
                        options,
                        allow_freeform,
                    });
                    wait_handle = Some(waiter);
                }
            }
            SessionEventKind::CompletionPublished => {
                if let SessionEventPayload::Completion { headline, body } = event.payload {
                    self.pending.cancel_session(&event.session_id);
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
                    self.pending.cancel_session(&event.session_id);
                    snapshot.status = SessionStatus::Failed;
                    snapshot.attention = None;
                    snapshot.last_summary = Some(format!("{headline}: {body}"));
                }
            }
            SessionEventKind::SessionEnded => {
                self.pending.cancel_session(&event.session_id);
                snapshot.status = SessionStatus::Ended;
                snapshot.attention = None;
            }
        }

        self.registry.upsert(snapshot);
        self.persist_state()?;

        if expect_blocking_waiter {
            Ok(wait_handle)
        } else {
            Ok(None)
        }
    }

    pub fn cleanup_request(
        &mut self,
        request_id: &RequestId,
        next_status: SessionStatus,
    ) -> Result<(), String> {
        self.pending.abandon_request(request_id);
        for snapshot in self.registry.all() {
            if snapshot
                .attention
                .as_ref()
                .is_some_and(|attention| &attention.request_id == request_id)
            {
                let mut updated = snapshot.clone();
                updated.status = next_status.clone();
                updated.attention = None;
                updated.last_updated_at = current_unix_timestamp();
                self.registry.upsert(updated);
                break;
            }
        }
        self.persist_state()
    }

    pub fn apply_ui_action(&mut self, action: UiAction) -> Result<Option<HookDecision>, String> {
        let decision = match action {
            UiAction::Approve { request_id, always } => {
                // TTY injection for Copilot tool approvals
                let session_id = self.pending.session_for_request(&request_id);
                if let Some(sid) = &session_id {
                    // For Copilot, "approve" = select first option (Allow)
                    match self.copilot_ttys.inject_answer(sid, "allow") {
                        Ok(true) => log_debug!("[tty] injected approval for session {}", sid.as_str()),
                        Ok(false) => {}
                        Err(e) => log_error!("[tty] approval injection failed: {e}"),
                    }
                }
                self.pending.resolve_approval(
                    &request_id,
                    if always {
                        ApprovalBehavior::AllowAlways
                    } else {
                        ApprovalBehavior::Allow
                    },
                )
            }
            UiAction::Deny { request_id } => {
                // TTY injection for Copilot tool denials
                let session_id = self.pending.session_for_request(&request_id);
                if let Some(sid) = &session_id {
                    // For Copilot, "deny" = select second option (index 1)
                    match self.copilot_ttys.inject_answer(sid, "deny") {
                        Ok(true) => log_debug!("[tty] injected denial for session {}", sid.as_str()),
                        Ok(false) => {}
                        Err(e) => log_error!("[tty] denial injection failed: {e}"),
                    }
                }
                self.pending
                    .resolve_approval(&request_id, ApprovalBehavior::Deny)
            }
            UiAction::AnswerOption {
                request_id,
                option_id,
            } => {
                // Try TTY injection for Copilot sessions before resolving
                let session_id = self.pending.session_for_request(&request_id);
                if let Some(sid) = &session_id {
                    match self.copilot_ttys.inject_answer(sid, &option_id) {
                        Ok(true) => log_debug!("[tty] injected answer for session {}", sid.as_str()),
                        Ok(false) => log_warn!("[tty] no TTY entry for session {}", sid.as_str()),
                        Err(e) => log_error!("[tty] injection failed: {e}"),
                    }
                } else {
                    log_warn!("[tty] no session found for request {:?}", request_id.as_str());
                }
                self.pending
                    .resolve_answer(&request_id, UserAnswer::Option { option_id })
            }
            UiAction::AnswerText { request_id, text } => {
                let session_id = self.pending.session_for_request(&request_id);
                if let Some(sid) = &session_id {
                    match self.copilot_ttys.inject_freeform(sid, &text) {
                        Ok(true) => log_debug!("[tty] injected freeform for session {}", sid.as_str()),
                        Ok(false) => log_warn!("[tty] no TTY entry for session {}", sid.as_str()),
                        Err(e) => log_error!("[tty] freeform injection failed: {e}"),
                    }
                }
                self.pending
                    .resolve_answer(&request_id, UserAnswer::Text { value: text })
            }
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
            self.cleanup_request(&request_id, SessionStatus::Running)?;
        } else {
            self.persist_state()?;
        }

        Ok(decision)
    }

    pub fn state_snapshot(&mut self) -> UiStateSnapshot {
        self.registry.cleanup_expired(current_unix_timestamp());
        let mut sessions: Vec<SessionListItem> = self
            .registry
            .all()
            .into_iter()
            .map(|snapshot| SessionListItem {
                session_id: snapshot.session_id.clone(),
                source: snapshot.source.clone(),
                title: snapshot.title.clone(),
                status: snapshot.status.clone(),
                summary: snapshot.last_summary.clone(),
                last_updated_at: snapshot.last_updated_at,
            })
            .collect();
        sessions.sort_by_key(|item| {
            (
                item.status.priority_rank(),
                std::cmp::Reverse(item.last_updated_at),
                item.title.clone(),
            )
        });

        let attentions: Vec<AttentionCard> = collect_attention_sessions(self.registry.all())
            .into_iter()
            .filter_map(|snapshot| {
                let attention = snapshot.attention.as_ref()?;
                Some(AttentionCard {
                    session_id: snapshot.session_id.clone(),
                    request_id: attention.request_id.clone(),
                    source: snapshot.source.clone(),
                    title: snapshot.title.clone(),
                    status: snapshot.status.clone(),
                    message: attention.message.clone(),
                    options: attention.options.clone(),
                    allow_freeform: attention.allow_freeform,
                })
            })
            .collect();

        UiStateSnapshot {
            attentions,
            sessions,
            recent_completions: self.completions.clone(),
            usage: self.usage.clone(),
        }
    }

    pub fn report(&mut self) -> String {
        let state = self.state_snapshot();
        let top = state
            .attentions
            .first()
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
        let sanitized_sessions = state
            .sessions
            .into_iter()
            .map(|mut snapshot| {
                if snapshot.status.requires_attention() || snapshot.attention.is_some() {
                    snapshot.status = SessionStatus::Stale;
                    snapshot.attention = None;
                }
                snapshot
            })
            .collect();
        self.registry.replace_all(sanitized_sessions);
        self.completions = state.recent_completions;
        Ok(())
    }
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::PupkitDaemon;
    use crate::daemon::DaemonConfig;
    use crate::protocol::{
        RequestId, SessionEvent, SessionEventKind, SessionEventPayload, SessionId, SourceKind,
        UiAction,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config(name: &str) -> DaemonConfig {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("pupkit-daemon-{name}-{ts}-{}", std::process::id()));
        DaemonConfig {
            socket_path: root.join("pupkitd.sock"),
            state_path: root.join("daemon-state.json"),
        }
    }

    #[test]
    fn top_attention_prefers_approval_requests() {
        let mut daemon = PupkitDaemon::for_config(temp_config("priority"));
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
            snapshot.attentions.first().unwrap().session_id.as_str(),
            "session-approval"
        );
    }

    #[test]
    fn approve_action_clears_attention() {
        let mut daemon = PupkitDaemon::for_config(temp_config("approve"));
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
        assert!(daemon.state_snapshot().attentions.is_empty());
    }

    #[test]
    fn restore_state_sanitizes_unrecoverable_attention() {
        let config = temp_config("restore");
        let mut daemon = PupkitDaemon::for_config(config.clone());
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::ClaudeCode,
                    SessionId::new("session-approval"),
                    SessionEventKind::ApprovalRequested,
                )
                .with_title("approval")
                .with_payload(SessionEventPayload::ApprovalRequest {
                    request_id: RequestId::new("req-restore"),
                    tool_name: "Edit".to_string(),
                    tool_input_summary: "modify src/main.rs".to_string(),
                }),
            )
            .unwrap();
        daemon.persist_state().unwrap();

        let mut restored = PupkitDaemon::for_config(config);
        let snapshot = restored.state_snapshot();
        assert!(snapshot.attentions.is_empty());
        assert_eq!(
            snapshot.sessions[0].status,
            crate::protocol::SessionStatus::Stale
        );
    }

    #[test]
    fn cwd_derives_project_title_for_all_sources() {
        let mut daemon = PupkitDaemon::for_config(temp_config("cwd-title"));
        // Copilot session with default title + CWD → "Copilot · my-app"
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::Copilot,
                    SessionId::new("cp-1"),
                    SessionEventKind::SessionUpdated,
                )
                .with_cwd("/home/dev/projects/my-app".to_string()),
            )
            .unwrap();
        // Claude Code session with default title + CWD → "Claude Code · lang_learn"
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::ClaudeCode,
                    SessionId::new("cc-1"),
                    SessionEventKind::SessionUpdated,
                )
                .with_cwd("/Users/dev/git/lang_learn".to_string()),
            )
            .unwrap();
        // Codex session with default title + CWD → "Codex · tasks"
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::Codex,
                    SessionId::new("cx-1"),
                    SessionEventKind::SessionUpdated,
                )
                .with_cwd("/tmp/tasks".to_string()),
            )
            .unwrap();

        let snap = daemon.state_snapshot();
        let titles: Vec<_> = snap.sessions.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Copilot · my-app"), "got: {titles:?}");
        assert!(titles.contains(&"Claude Code · lang_learn"), "got: {titles:?}");
        assert!(titles.contains(&"Codex · tasks"), "got: {titles:?}");
    }

    #[test]
    fn explicit_title_preserved_over_cwd_derivation() {
        let mut daemon = PupkitDaemon::for_config(temp_config("cwd-explicit"));
        // Session with explicit title containing · should not be overwritten
        daemon
            .ingest_event(
                SessionEvent::new(
                    SourceKind::Copilot,
                    SessionId::new("cp-2"),
                    SessionEventKind::SessionStarted,
                )
                .with_title("Copilot · custom-name".to_string())
                .with_cwd("/home/dev/other-dir".to_string()),
            )
            .unwrap();

        let snap = daemon.state_snapshot();
        assert_eq!(snap.sessions[0].title, "Copilot · custom-name");
    }
}
