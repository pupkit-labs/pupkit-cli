use std::env;

use crate::daemon::{DaemonConfig, SessionRegistry, select_top_session};
use crate::protocol::SessionSnapshot;

#[derive(Clone, Debug)]
pub struct PupkitDaemon {
    config: DaemonConfig,
    registry: SessionRegistry,
}

impl PupkitDaemon {
    pub fn bootstrap() -> Self {
        let home = env::var_os("HOME").map(Into::into);
        Self {
            config: DaemonConfig::default_for_home(home),
            registry: SessionRegistry::default(),
        }
    }

    pub fn config(&self) -> &DaemonConfig {
        &self.config
    }

    pub fn upsert_session(&mut self, snapshot: SessionSnapshot) {
        self.registry.upsert(snapshot);
    }

    pub fn top_session(&self) -> Option<&SessionSnapshot> {
        select_top_session(self.registry.all())
    }

    pub fn report(&self) -> String {
        format!(
            "pupkit daemon scaffold ready\nsocket: {}\nstate: {}",
            self.config.socket_path.display(),
            self.config.state_path.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::PupkitDaemon;
    use crate::protocol::{SessionId, SessionSnapshot, SessionStatus, SourceKind};

    #[test]
    fn top_session_prefers_attention_states() {
        let mut daemon = PupkitDaemon::bootstrap();
        daemon.upsert_session(SessionSnapshot::new(
            SessionId::new("running"),
            SourceKind::ClaudeCode,
            "running".to_string(),
            SessionStatus::Running,
        ));
        daemon.upsert_session(SessionSnapshot::new(
            SessionId::new("waiting"),
            SourceKind::Codex,
            "waiting".to_string(),
            SessionStatus::WaitingApproval,
        ));

        assert_eq!(daemon.top_session().unwrap().session_id.as_str(), "waiting");
    }
}
