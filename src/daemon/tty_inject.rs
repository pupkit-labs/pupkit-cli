//! TTY keystroke injection for Copilot `ask_user` responses.
//!
//! When the watcher detects a Copilot `ask_user` tool call, we discover the
//! process's controlling TTY. When the user clicks an option in the Dynamic
//! Island, we inject arrow-key sequences into the TTY to select the answer.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::protocol::SessionId;

/// Stores TTY paths and choice lists for active Copilot ask_user prompts.
#[derive(Debug, Default)]
pub struct CopilotTtyStore {
    entries: HashMap<SessionId, TtyEntry>,
}

#[derive(Debug)]
struct TtyEntry {
    pub tty_path: PathBuf,
    pub choices: Vec<String>,
}

impl CopilotTtyStore {
    pub fn set(&mut self, session_id: SessionId, tty_path: PathBuf, choices: Vec<String>) {
        self.entries.insert(session_id, TtyEntry { tty_path, choices });
    }

    pub fn remove(&mut self, session_id: &SessionId) {
        self.entries.remove(session_id);
    }

    /// Inject a choice selection into the Copilot TTY.
    ///
    /// Returns Ok(true) if injection was performed, Ok(false) if no TTY entry found.
    pub fn inject_answer(
        &mut self,
        session_id: &SessionId,
        option_text: &str,
    ) -> Result<bool, String> {
        let entry = match self.entries.remove(session_id) {
            Some(e) => e,
            None => return Ok(false),
        };

        let choice_index = entry
            .choices
            .iter()
            .position(|c| c == option_text)
            .unwrap_or(0);

        inject_choice(&entry.tty_path, choice_index)
            .map_err(|e| format!("TTY inject failed: {e}"))?;

        Ok(true)
    }
}

// MARK: - TTY Discovery

/// Discover the TTY device for a Copilot session by reading its lock file.
///
/// Steps:
/// 1. Find `inuse.<pid>.lock` in the session directory
/// 2. Read the PID from the lock file
/// 3. Use `lsof` to find the TTY device for stdin (fd 0)
pub fn discover_tty(session_dir: &Path) -> Option<PathBuf> {
    let pid = read_pid_from_lock(session_dir)?;
    find_tty_for_pid(pid)
}

fn read_pid_from_lock(session_dir: &Path) -> Option<u32> {
    let entries = fs::read_dir(session_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("inuse.") && name_str.ends_with(".lock") {
            let pid_str = name_str
                .strip_prefix("inuse.")?
                .strip_suffix(".lock")?;
            return pid_str.parse().ok();
        }
    }
    None
}

fn find_tty_for_pid(pid: u32) -> Option<PathBuf> {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string(), "-a", "-d", "0", "-F", "n"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix('n') {
            if path.starts_with("/dev/tty") {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

// MARK: - Keystroke Injection

/// Inject a choice selection by sending arrow-down keys and Enter to the TTY.
///
/// The Copilot `ask_user` TUI starts with the first option selected (index 0).
/// To select option N, we send N down-arrow sequences, then Enter.
fn inject_choice(tty_path: &Path, choice_index: usize) -> std::io::Result<()> {
    let mut tty = OpenOptions::new().write(true).open(tty_path)?;

    // Send down-arrow keys: ESC [ B
    for _ in 0..choice_index {
        tty.write_all(b"\x1b[B")?;
    }

    // Small flush between arrows and enter for TUI to process
    tty.flush()?;

    // Send Enter
    tty.write_all(b"\r")?;
    tty.flush()?;

    Ok(())
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn read_pid_from_lock_extracts_pid() {
        let dir = std::env::temp_dir().join(format!("pupkit-tty-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let lock_file = dir.join("inuse.12345.lock");
        fs::File::create(&lock_file)
            .unwrap()
            .write_all(b"12345")
            .unwrap();

        assert_eq!(read_pid_from_lock(&dir), Some(12345));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_pid_from_lock_returns_none_for_empty_dir() {
        let dir = std::env::temp_dir().join(format!("pupkit-tty-empty-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();

        assert_eq!(read_pid_from_lock(&dir), None);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copilot_tty_store_set_and_inject() {
        let mut store = CopilotTtyStore::default();
        let sid = SessionId::new("test-session");

        // Set with a nonexistent TTY — inject will fail but the lookup should work
        store.set(
            sid.clone(),
            PathBuf::from("/dev/null"),
            vec!["Option A".into(), "Option B".into(), "Option C".into()],
        );

        // After inject (even failed), entry should be removed
        let result = store.inject_answer(&sid, "Option B");
        // /dev/null accepts writes, so this should succeed
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);

        // Entry should be consumed
        assert!(store.inject_answer(&sid, "Option A").unwrap() == false);
    }

    #[test]
    fn copilot_tty_store_returns_false_for_unknown_session() {
        let mut store = CopilotTtyStore::default();
        let sid = SessionId::new("unknown");
        assert_eq!(store.inject_answer(&sid, "whatever").unwrap(), false);
    }
}
