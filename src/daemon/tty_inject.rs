//! TTY keystroke injection for Copilot `ask_user` responses.
//!
//! When the watcher detects a Copilot `ask_user` tool call, we discover the
//! process's controlling TTY. When the user clicks an option in the Dynamic
//! Island, we inject arrow-key sequences into the TTY to select the answer.

use std::collections::HashMap;
use std::fs;
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

    /// Inject a freeform text answer into the Copilot TTY.
    ///
    /// Navigates past all choices to the text input, types the text, then submits.
    /// Returns Ok(true) if injection was performed, Ok(false) if no TTY entry found.
    pub fn inject_freeform(
        &mut self,
        session_id: &SessionId,
        text: &str,
    ) -> Result<bool, String> {
        let entry = match self.entries.remove(session_id) {
            Some(e) => e,
            None => return Ok(false),
        };

        inject_freeform_text(&entry.tty_path, entry.choices.len(), text)
            .map_err(|e| format!("TTY freeform inject failed: {e}"))?;

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

/// Inject a choice selection via osascript + iTerm2.
///
/// Direct TTY writes go to the output side, not input. On macOS 15+, TIOCSTI
/// is blocked. So we use iTerm2's AppleScript API to send keystrokes to the
/// specific session identified by its TTY device path.
///
/// The Copilot `ask_user` TUI starts with the first option selected (index 0).
/// To select option N, we send N down-arrow sequences, then Enter.
fn inject_choice(tty_path: &Path, choice_index: usize) -> std::io::Result<()> {
    let tty_str = tty_path.to_string_lossy();

    // Build the AppleScript to send keystrokes to the right iTerm2 session
    let mut arrow_commands = String::new();
    for _ in 0..choice_index {
        arrow_commands.push_str(
            "                    tell s to write text (character id 27) & \"[B\" without newline\n\
             \x20                   delay 0.05\n",
        );
    }

    let script = format!(
        r#"tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if tty of s is "{tty}" then
{arrows}                    tell s to write text (character id 13) without newline
                    return "ok"
                end if
            end repeat
        end repeat
    end repeat
    return "session not found"
end tell"#,
        tty = tty_str,
        arrows = arrow_commands,
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()?;

    let result = String::from_utf8_lossy(&output.stdout);
    if result.trim() != "ok" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("iTerm2 session not found for {tty_str}: {result}"),
        ));
    }

    Ok(())
}

/// Inject freeform text by navigating past all choices to the text input area,
/// typing the text, then pressing Enter.
///
/// In Copilot's ask_user TUI, the freeform input is below the choices list.
/// We need `num_choices` down-arrows to get past all options to the text field.
fn inject_freeform_text(tty_path: &Path, num_choices: usize, text: &str) -> std::io::Result<()> {
    let tty_str = tty_path.to_string_lossy();

    // Build down-arrow commands to skip past all choices to the freeform input
    let mut arrow_commands = String::new();
    for _ in 0..num_choices {
        arrow_commands.push_str(
            "                    tell s to write text (character id 27) & \"[B\" without newline\n\
             \x20                   delay 0.05\n",
        );
    }

    // Escape the text for AppleScript string (double any backslashes and quotes)
    let escaped_text = text.replace('\\', "\\\\").replace('"', "\\\"");

    let script = format!(
        r#"tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if tty of s is "{tty}" then
{arrows}                    delay 0.1
                    tell s to write text "{text}" without newline
                    delay 0.05
                    tell s to write text (character id 13) without newline
                    return "ok"
                end if
            end repeat
        end repeat
    end repeat
    return "session not found"
end tell"#,
        tty = tty_str,
        arrows = arrow_commands,
        text = escaped_text,
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()?;

    let result = String::from_utf8_lossy(&output.stdout);
    if result.trim() != "ok" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("iTerm2 session not found for {tty_str}: {result}"),
        ));
    }

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

        store.set(
            sid.clone(),
            PathBuf::from("/dev/null"),
            vec!["Option A".into(), "Option B".into(), "Option C".into()],
        );

        // osascript won't find an iTerm2 session for /dev/null — inject returns Err
        let result = store.inject_answer(&sid, "Option B");
        assert!(result.is_err());

        // Entry should be consumed after inject attempt (removed before injection)
        assert_eq!(store.inject_answer(&sid, "Option A").unwrap(), false);
    }

    #[test]
    fn copilot_tty_store_returns_false_for_unknown_session() {
        let mut store = CopilotTtyStore::default();
        let sid = SessionId::new("unknown");
        assert_eq!(store.inject_answer(&sid, "whatever").unwrap(), false);
        assert_eq!(store.inject_freeform(&sid, "hello").unwrap(), false);
    }

    #[test]
    fn copilot_tty_store_inject_freeform() {
        let mut store = CopilotTtyStore::default();
        let sid = SessionId::new("freeform-session");

        store.set(
            sid.clone(),
            PathBuf::from("/dev/null"),
            vec!["A".into(), "B".into()],
        );

        // osascript won't find /dev/null — inject returns Err
        let result = store.inject_freeform(&sid, "custom text");
        assert!(result.is_err());

        // Entry consumed
        assert_eq!(store.inject_freeform(&sid, "anything").unwrap(), false);
    }
}
