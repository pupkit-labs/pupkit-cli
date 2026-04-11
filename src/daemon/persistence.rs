use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::protocol::{CompletionItem, SessionSnapshot};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PersistentDaemonState {
    pub sessions: Vec<SessionSnapshot>,
    pub recent_completions: Vec<CompletionItem>,
}

pub fn load_state(path: &Path) -> Result<PersistentDaemonState, String> {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content)
            .map_err(|error| format!("failed to parse daemon state: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PersistentDaemonState::default())
        }
        Err(error) => Err(format!("failed to read daemon state: {error}")),
    }
}

pub fn save_state(path: &Path, state: &PersistentDaemonState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create daemon state dir: {error}"))?;
    }
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("failed to serialize daemon state: {error}"))?;
    fs::write(path, content).map_err(|error| format!("failed to write daemon state: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{PersistentDaemonState, load_state, save_state};
    use crate::protocol::{CompletionItem, SessionId, SessionSnapshot, SessionStatus, SourceKind};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pupkit-{name}-{ts}-{}.json", std::process::id()))
    }

    #[test]
    fn saves_and_loads_state_roundtrip() {
        let path = temp_path("daemon-state");
        let state = PersistentDaemonState {
            sessions: vec![SessionSnapshot::new(
                SessionId::new("session-1"),
                SourceKind::ClaudeCode,
                "demo".to_string(),
                SessionStatus::Running,
            )],
            recent_completions: vec![CompletionItem {
                session_id: SessionId::new("session-1"),
                source: SourceKind::ClaudeCode,
                title: "demo".to_string(),
                headline: "done".to_string(),
                body: "body".to_string(),
            }],
        };

        save_state(&path, &state).unwrap();
        let restored = load_state(&path).unwrap();

        assert_eq!(restored, state);
        let _ = std::fs::remove_file(path);
    }
}
