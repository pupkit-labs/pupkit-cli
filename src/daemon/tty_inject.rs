//! TTY keystroke injection for Copilot `ask_user` responses.
//!
//! When the watcher detects a Copilot `ask_user` tool call, we discover the
//! process's controlling TTY. When the user clicks an option in the Dynamic
//! Island, we inject arrow-key sequences into the TTY to select the answer.
//!
//! Supports iTerm2 (preferred) and Terminal.app as injection backends.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::protocol::{SessionId, SourceKind};
use crate::log_warn;

/// Stores TTY paths and choice lists for active tool-approval prompts.
///
/// Supports Copilot (arrow-key navigation), Claude Code & Codex (y/n text).
#[derive(Debug, Default)]
pub struct CopilotTtyStore {
    entries: HashMap<SessionId, TtyEntry>,
}

#[derive(Debug)]
struct TtyEntry {
    pub tty_path: PathBuf,
    pub choices: Vec<String>,
    pub source: SourceKind,
}

impl CopilotTtyStore {
    pub fn set(&mut self, session_id: SessionId, tty_path: PathBuf, choices: Vec<String>, source: SourceKind) {
        self.entries.insert(session_id, TtyEntry { tty_path, choices, source });
    }

    pub fn remove(&mut self, session_id: &SessionId) {
        self.entries.remove(session_id);
    }

    /// Inject a choice selection into the TTY.
    ///
    /// For Copilot: arrow-key navigation in TUI list.
    /// For Claude Code / Codex: simple "y" or "n" text injection.
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

        match entry.source {
            SourceKind::ClaudeCode => {
                // Claude Code uses inquirer-style TUI list:
                //   ❯ 1. Yes              (index 0 — default)
                //     2. Yes, allow all    (index 1)
                //     3. No               (index 2 — last)
                let choice_index = match option_text {
                    "allow" => 0,
                    "deny" => entry.choices.len().saturating_sub(1).max(1),
                    _ => 0,
                };
                inject_choice(&entry.tty_path, choice_index)
                    .map_err(|e| format!("TTY inject failed: {e}"))?;
            }
            SourceKind::Codex => {
                let choice_index = entry
                    .choices
                    .iter()
                    .position(|c| c == option_text)
                    .unwrap_or(0);
                inject_choice(&entry.tty_path, choice_index)
                    .map_err(|e| format!("TTY inject failed: {e}"))?;
            }
            _ => {
                // Copilot-style arrow navigation
                let choice_index = entry
                    .choices
                    .iter()
                    .position(|c| c == option_text)
                    .unwrap_or(0);
                inject_choice(&entry.tty_path, choice_index)
                    .map_err(|e| format!("TTY inject failed: {e}"))?;
            }
        }
        Ok(true)
    }

    /// Inject a freeform text answer into the TTY.
    ///
    /// For Copilot: navigates past all choices to the text input, types, submits.
    /// For Claude Code / Codex: directly types the text and presses Enter.
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

        match entry.source {
            SourceKind::ClaudeCode => {
                // Claude Code also uses TUI lists; navigate past choices then type
                inject_freeform_text(&entry.tty_path, entry.choices.len(), text)
                    .map_err(|e| format!("TTY freeform inject failed: {e}"))?;
            }
            _ => {
                // Copilot & Codex: navigate past TUI choices, then type
                inject_freeform_text(&entry.tty_path, entry.choices.len(), text)
                    .map_err(|e| format!("TTY freeform inject failed: {e}"))?;
            }
        }
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

/// Discover the TTY device for a Claude Code / Codex session via its JSONL file.
///
/// Claude Code doesn't keep the JSONL file descriptor open, so `lsof <file>`
/// won't find the process. Instead we:
/// 1. Extract the sanitised cwd from the JSONL parent directory name
///    (e.g. `-Users-pengxu-liu-project` → `/Users/pengxu/liu/project`)
/// 2. Find all `claude` / `codex` processes via `pgrep`
/// 3. Match the process whose cwd equals the extracted path
/// 4. Resolve the TTY via `find_tty_for_pid`
///
/// Falls back to `lsof <file>` for the rare case where the fd is still held.
pub fn discover_tty_from_jsonl(jsonl_path: &Path) -> Option<PathBuf> {
    // Strategy 1: match process cwd to the JSONL directory name
    if let Some(tty) = discover_tty_by_cwd(jsonl_path) {
        return Some(tty);
    }

    // Strategy 2: fallback — lsof on the file itself (works if fd still open)
    let output = Command::new("lsof")
        .arg(jsonl_path.to_string_lossy().as_ref())
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(pid) = parts[1].parse::<u32>() {
                if let Some(tty) = find_tty_for_pid(pid) {
                    return Some(tty);
                }
            }
        }
    }
    None
}

/// Extract the original cwd from a Claude/Codex JSONL parent directory name.
///
/// Directory names look like `-Users-pengxu-liu-project` which maps to
/// `/Users/pengxu.liu/project`. The heuristic: replace leading `-` with `/`
/// and internal `-` with `/`, then check if the path exists. If not, this
/// won't match and we fall through to the lsof fallback.
fn cwd_from_jsonl_dir(jsonl_path: &Path) -> Option<String> {
    let dir_name = jsonl_path.parent()?.file_name()?.to_str()?;
    // The directory name is the cwd with `/` replaced by `-`
    // e.g. `-Users-pengxu-liu-liupx-git-study-lang_extract_learning`
    // → `/Users/pengxu.liu/liupx/git/study/lang_extract_learning`
    // But `.` in usernames becomes `-` too, so exact reversal is ambiguous.
    // We'll collect candidate PIDs and match by checking their actual cwd.
    Some(dir_name.to_string())
}

/// Find a claude/codex process whose cwd matches the JSONL directory.
fn discover_tty_by_cwd(jsonl_path: &Path) -> Option<PathBuf> {
    let dir_name = cwd_from_jsonl_dir(jsonl_path)?;

    // Claude sanitises cwd by replacing `/`, `.`, `_` and other chars with `-`.
    // We normalise both sides the same way before comparing.
    let normalised_dir = normalise_for_cwd_match(&dir_name);

    // Get PIDs of claude and codex processes
    let mut pids = Vec::new();
    for name in &["claude", "codex"] {
        if let Ok(output) = Command::new("pgrep").arg("-x").arg(name).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
    }

    for pid in pids {
        // Get the cwd of this process
        if let Ok(cwd_output) = Command::new("lsof")
            .args(["-p", &pid.to_string(), "-a", "-d", "cwd", "-F", "n"])
            .output()
        {
            let cwd_stdout = String::from_utf8_lossy(&cwd_output.stdout);
            for line in cwd_stdout.lines() {
                if let Some(cwd_path) = line.strip_prefix('n') {
                    // Sanitise the cwd: replace `/` with `-`, then normalise `.` and `_`
                    let sanitised = cwd_path.replace('/', "-");
                    if normalise_for_cwd_match(&sanitised) == normalised_dir {
                        return find_tty_for_pid(pid);
                    }
                }
            }
        }
    }
    None
}

/// Normalise a sanitised path for fuzzy cwd matching.
/// Claude replaces `/`, `.`, `_`, and possibly other chars with `-`.
fn normalise_for_cwd_match(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '.' | '_' | '/' => '-',
            _ => c,
        })
        .collect()
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

/// Which terminal emulator to target for keystroke injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalKind {
    ITerm2,
    TerminalApp,
}

/// Detect which terminal emulators are currently running.
fn detect_running_terminals() -> Vec<TerminalKind> {
    let mut terminals = Vec::new();
    for (name, kind) in &[("iTerm2", TerminalKind::ITerm2), ("Terminal", TerminalKind::TerminalApp)] {
        if Command::new("pgrep")
            .args(["-x", name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            terminals.push(*kind);
        }
    }
    terminals
}

/// Inject a choice selection via the first available terminal backend.
///
/// Tries iTerm2 first (richer API), then Terminal.app via System Events.
/// The TUI starts with the first option selected (index 0).
/// To select option N, we send N down-arrow sequences, then Enter.
fn inject_choice(tty_path: &Path, choice_index: usize) -> std::io::Result<()> {
    let terminals = detect_running_terminals();
    let mut last_err = None;

    for terminal in &terminals {
        match terminal {
            TerminalKind::ITerm2 => {
                match inject_choice_iterm2(tty_path, choice_index) {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
            }
            TerminalKind::TerminalApp => {
                match inject_choice_terminal_app(tty_path, choice_index) {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no supported terminal emulator running (iTerm2 or Terminal.app required)",
    )))
}

/// Inject freeform text by navigating past all choices to the text input area,
/// typing the text, then pressing Enter.
fn inject_freeform_text(tty_path: &Path, num_choices: usize, text: &str) -> std::io::Result<()> {
    let terminals = detect_running_terminals();
    let mut last_err = None;

    for terminal in &terminals {
        match terminal {
            TerminalKind::ITerm2 => {
                match inject_freeform_iterm2(tty_path, num_choices, text) {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
            }
            TerminalKind::TerminalApp => {
                match inject_freeform_terminal_app(tty_path, num_choices, text) {
                    Ok(()) => return Ok(()),
                    Err(e) => last_err = Some(e),
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no supported terminal emulator running (iTerm2 or Terminal.app required)",
    )))
}

// MARK: - iTerm2 Backend

/// Inject a choice selection via iTerm2's AppleScript API.
fn inject_choice_iterm2(tty_path: &Path, choice_index: usize) -> std::io::Result<()> {
    let tty_str = tty_path.to_string_lossy();

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

    run_osascript_expect_ok(&script, &tty_str, "iTerm2")
}

/// Inject freeform text via iTerm2's AppleScript API.
fn inject_freeform_iterm2(tty_path: &Path, num_choices: usize, text: &str) -> std::io::Result<()> {
    let tty_str = tty_path.to_string_lossy();
    let escaped_text = text.replace('\\', "\\\\").replace('"', "\\\"");

    let mut arrow_commands = String::new();
    for _ in 0..num_choices {
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
{arrows}                    delay 0.1
                    tell s to write text (character id 13) without newline
                    delay 0.15
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

    run_osascript_expect_ok(&script, &tty_str, "iTerm2")
}

// MARK: - Terminal.app Backend

/// Inject a choice selection via Terminal.app + System Events.
///
/// Terminal.app doesn't support direct text injection into a session.
/// We find the window/tab by TTY path, bring it to front, then use
/// System Events to send arrow key codes and `keystroke return`.
///
/// NOTE: `keystroke return` is required instead of `key code 36` because
/// terminal TUI frameworks (inquirer/crossterm) don't respond to raw
/// key-code events — they need the character-level Return event.
fn inject_choice_terminal_app(tty_path: &Path, choice_index: usize) -> std::io::Result<()> {
    let tty_str = tty_path.to_string_lossy();

    let mut key_commands = String::new();
    for _ in 0..choice_index {
        key_commands.push_str(
            "                            key code 125\n\
             \x20                           delay 0.05\n",
        );
    }

    let script = format!(
        r#"tell application "Terminal"
    activate
    repeat with w in windows
        set tabCount to count of tabs of w
        repeat with i from 1 to tabCount
            set t to tab i of w
            if tty of t is "{tty}" then
                set selected tab of w to t
                set frontmost of w to true
                delay 0.3
                tell application "System Events"
                    tell process "Terminal"
{keys}                            keystroke return
                    end tell
                end tell
                return "ok"
            end if
        end repeat
    end repeat
    return "session not found"
end tell"#,
        tty = tty_str,
        keys = key_commands,
    );

    run_osascript_expect_ok(&script, &tty_str, "Terminal.app")
}

/// Inject freeform text via Terminal.app + System Events.
///
/// Uses clipboard paste (Cmd+V) for the text content because AppleScript's
/// `keystroke` doesn't support non-ASCII characters (Chinese, Japanese, etc.).
/// The original clipboard content is saved and restored after pasting.
fn inject_freeform_terminal_app(tty_path: &Path, num_choices: usize, text: &str) -> std::io::Result<()> {
    let tty_str = tty_path.to_string_lossy();
    let escaped_text = text.replace('\\', "\\\\").replace('"', "\\\"");

    let mut arrow_commands = String::new();
    for _ in 0..num_choices {
        arrow_commands.push_str(
            "                            key code 125\n\
             \x20                           delay 0.05\n",
        );
    }

    let script = format!(
        r#"-- Save clipboard, paste text, restore clipboard
set oldClip to the clipboard
set the clipboard to "{text}"
tell application "Terminal"
    activate
    repeat with w in windows
        set tabCount to count of tabs of w
        repeat with i from 1 to tabCount
            set t to tab i of w
            if tty of t is "{tty}" then
                set selected tab of w to t
                set frontmost of w to true
                delay 0.3
                tell application "System Events"
                    tell process "Terminal"
{arrows}                            delay 0.1
                            keystroke return
                            delay 0.15
                            keystroke "v" using command down
                            delay 0.05
                            keystroke return
                    end tell
                end tell
                set the clipboard to oldClip
                return "ok"
            end if
        end repeat
    end repeat
    return "session not found"
end tell"#,
        tty = tty_str,
        arrows = arrow_commands,
        text = escaped_text,
    );

    run_osascript_expect_ok(&script, &tty_str, "Terminal.app")
}

// MARK: - Shared Helpers

/// Run an AppleScript and expect "ok" as the return value.
fn run_osascript_expect_ok(script: &str, tty_str: &str, terminal_name: &str) -> std::io::Result<()> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()?;

    let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        log_warn!("[tty] osascript stderr for {terminal_name}: {stderr}");
    }
    if result == "ok" {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{terminal_name} session not found for {tty_str}: {result}"),
        ))
    }
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
            SourceKind::Copilot,
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
            SourceKind::Copilot,
        );

        // osascript won't find /dev/null — inject returns Err
        let result = store.inject_freeform(&sid, "custom text");
        assert!(result.is_err());

        // Entry consumed
        assert_eq!(store.inject_freeform(&sid, "anything").unwrap(), false);
    }
}
