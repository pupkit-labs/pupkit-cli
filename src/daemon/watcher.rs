use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::daemon::PupkitDaemon;
use crate::daemon::tty_inject;
use crate::protocol::{
    RequestId, SessionEvent, SessionEventKind, SessionEventPayload, SessionId, SourceKind,
};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Only process files modified within this window (avoids scanning ancient history).
const MAX_FILE_AGE_SECS: u64 = 24 * 60 * 60;

// MARK: - Watch sources

#[derive(Clone, Debug)]
struct WatchSource {
    root: PathBuf,
    kind: WatchSourceKind,
}

#[derive(Clone, Debug)]
enum WatchSourceKind {
    Claude,
    Codex,
    Copilot,
}

// MARK: - File cursor

#[derive(Default)]
struct FileCursors {
    positions: HashMap<PathBuf, u64>,
}

impl FileCursors {
    fn get_position(&self, path: &Path) -> u64 {
        self.positions.get(path).copied().unwrap_or(0)
    }

    fn set_position(&mut self, path: PathBuf, pos: u64) {
        self.positions.insert(path, pos);
    }

    /// Remove entries for files that no longer exist.
    fn prune_missing(&mut self) {
        self.positions.retain(|path, _| path.exists());
    }
}

// MARK: - Watcher

pub fn spawn_watcher(daemon: Arc<Mutex<PupkitDaemon>>, home: PathBuf) {
    thread::Builder::new()
        .name("watcher".to_string())
        .spawn(move || watcher_loop(daemon, home))
        .expect("failed to spawn watcher thread");
}

fn watcher_loop(daemon: Arc<Mutex<PupkitDaemon>>, home: PathBuf) {
    let sources = discover_sources(&home);
    let mut cursors = FileCursors::default();

    // On first run, seek all existing files to end so we only see new events.
    initialize_cursors(&sources, &mut cursors);

    loop {
        thread::sleep(POLL_INTERVAL);

        let events = poll_all_sources(&sources, &mut cursors);
        if events.is_empty() {
            continue;
        }

        if let Ok(mut daemon) = daemon.lock() {
            for event in events {
                // For Copilot QuestionRequested events, discover and register the TTY
                if event.kind == SessionEventKind::QuestionRequested {
                    if let SessionEventPayload::QuestionRequest { ref options, .. } = event.payload
                    {
                        // Derive session dir from the Copilot root
                        let copilot_root = home.join(".copilot/session-state");
                        let session_dir = copilot_root.join(event.session_id.as_str());
                        match tty_inject::discover_tty(&session_dir) {
                            Some(tty) => {
                                eprintln!("[watcher] TTY discovered for {}: {}", event.session_id.as_str(), tty.display());
                                daemon.copilot_ttys_mut().set(
                                    event.session_id.clone(),
                                    tty,
                                    options.clone(),
                                );
                            }
                            None => {
                                eprintln!("[watcher] TTY discovery failed for session dir: {}", session_dir.display());
                            }
                        }
                    }
                }
                let _ = daemon.ingest_event(event);
            }
        }

        // Periodically prune cursors for deleted files
        cursors.prune_missing();
    }
}

fn discover_sources(home: &Path) -> Vec<WatchSource> {
    let mut sources = Vec::new();

    let claude_root = home.join(".claude/projects");
    if claude_root.is_dir() {
        sources.push(WatchSource {
            root: claude_root,
            kind: WatchSourceKind::Claude,
        });
    }

    let codex_root = home.join(".codex/sessions");
    if codex_root.is_dir() {
        sources.push(WatchSource {
            root: codex_root,
            kind: WatchSourceKind::Codex,
        });
    }

    let copilot_root = home.join(".copilot/session-state");
    if copilot_root.is_dir() {
        sources.push(WatchSource {
            root: copilot_root,
            kind: WatchSourceKind::Copilot,
        });
    }

    sources
}

fn initialize_cursors(sources: &[WatchSource], cursors: &mut FileCursors) {
    for source in sources {
        let files = find_jsonl_files(&source.root, &source.kind);
        for path in files {
            if let Ok(meta) = fs::metadata(&path) {
                cursors.set_position(path, meta.len());
            }
        }
    }
}

fn poll_all_sources(sources: &[WatchSource], cursors: &mut FileCursors) -> Vec<SessionEvent> {
    let mut events = Vec::new();
    let now = current_epoch_secs();

    for source in sources {
        let files = find_jsonl_files(&source.root, &source.kind);
        for path in files {
            if !is_recently_modified(&path, now) {
                continue;
            }
            let new_lines = read_new_lines(&path, cursors);
            for line in new_lines {
                if let Some(event) = parse_line(&line, &source.kind, &path) {
                    events.push(event);
                }
            }
        }
    }

    events
}

// MARK: - File discovery

fn find_jsonl_files(root: &Path, kind: &WatchSourceKind) -> Vec<PathBuf> {
    let mut results = Vec::new();
    match kind {
        WatchSourceKind::Copilot => {
            // ~/.copilot/session-state/<id>/events.jsonl
            if let Ok(entries) = fs::read_dir(root) {
                for entry in entries.flatten() {
                    let events_file = entry.path().join("events.jsonl");
                    if events_file.is_file() {
                        results.push(events_file);
                    }
                }
            }
        }
        WatchSourceKind::Claude | WatchSourceKind::Codex => {
            // Recursive scan for *.jsonl
            collect_jsonl_recursive(root, &mut results, 5);
        }
    }
    results
}

fn collect_jsonl_recursive(dir: &Path, results: &mut Vec<PathBuf>, depth: u8) {
    if depth == 0 {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_recursive(&path, results, depth - 1);
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            results.push(path);
        }
    }
}

// MARK: - Line reading

fn read_new_lines(path: &Path, cursors: &mut FileCursors) -> Vec<String> {
    let last_pos = cursors.get_position(path);
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file_len <= last_pos {
        // File hasn't grown (or was truncated — reset to current size)
        if file_len < last_pos {
            cursors.set_position(path.to_path_buf(), file_len);
        }
        return Vec::new();
    }

    let mut reader = BufReader::new(file);
    if reader.seek(SeekFrom::Start(last_pos)).is_err() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut new_pos = last_pos;
    let mut buf = String::new();
    while reader.read_line(&mut buf).unwrap_or(0) > 0 {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
        new_pos = reader.stream_position().unwrap_or(new_pos);
        buf.clear();
    }

    cursors.set_position(path.to_path_buf(), new_pos);
    lines
}

// MARK: - Line parsers

fn parse_line(line: &str, kind: &WatchSourceKind, path: &Path) -> Option<SessionEvent> {
    let value: Value = serde_json::from_str(line).ok()?;
    match kind {
        WatchSourceKind::Claude => parse_claude_line(&value, path),
        WatchSourceKind::Codex => parse_codex_line(&value, path),
        WatchSourceKind::Copilot => parse_copilot_line(&value, path),
    }
}

/// Parse a Claude Code JSONL line into a SessionEvent.
///
/// Claude JSONL has types: user, assistant, system, progress, file-history-snapshot.
/// We generate:
/// - `user` (first in file) → SessionStarted
/// - `user` / `assistant` → SessionUpdated
/// - `user` with /exit content → SessionEnded
fn parse_claude_line(value: &Value, path: &Path) -> Option<SessionEvent> {
    let line_type = value.get("type")?.as_str()?;
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| session_id_from_path(path));

    let cwd = value.get("cwd").and_then(|v| v.as_str()).map(String::from);
    let occurred_at = extract_timestamp(value);

    match line_type {
        "user" => {
            // Check for exit command
            let content = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if content.contains("/exit") || content.contains("command-name>/exit") {
                let mut event = SessionEvent::new(
                    SourceKind::ClaudeCode,
                    SessionId::new(&session_id),
                    SessionEventKind::SessionEnded,
                );
                if let Some(cwd) = cwd {
                    event = event.with_cwd(cwd);
                }
                event = event.with_occurred_at(occurred_at);
                return Some(event);
            }

            let mut event = SessionEvent::new(
                SourceKind::ClaudeCode,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            );
            // Extract title from slug field
            if let Some(slug) = value.get("slug").and_then(|v| v.as_str()) {
                event = event.with_title(slug.replace('-', " "));
            }
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd);
            }
            event = event.with_occurred_at(occurred_at);
            Some(event)
        }
        "assistant" => {
            let mut event = SessionEvent::new(
                SourceKind::ClaudeCode,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            );
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd);
            }
            event = event.with_occurred_at(occurred_at);
            // Extract summary from assistant text content
            if let Some(summary) = extract_assistant_summary(value) {
                event = event.with_summary(summary);
            }
            Some(event)
        }
        _ => None, // Skip system, progress, file-history-snapshot
    }
}

/// Parse a Codex JSONL line.
///
/// Codex has types: session_meta, event_msg, response_item, turn_context, compacted.
/// We generate:
/// - `session_meta` → SessionStarted
/// - `event_msg` with payload.type=="task_started" → SessionUpdated
fn parse_codex_line(value: &Value, path: &Path) -> Option<SessionEvent> {
    let line_type = value.get("type")?.as_str()?;
    let occurred_at = extract_timestamp(value);

    match line_type {
        "session_meta" => {
            let payload = value.get("payload")?;
            let session_id = payload
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| session_id_from_path(path));
            let cwd = payload
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(String::from);
            let model = payload
                .get("model_provider")
                .and_then(|v| v.as_str())
                .unwrap_or("Codex");

            let mut event = SessionEvent::new(
                SourceKind::Codex,
                SessionId::new(&session_id),
                SessionEventKind::SessionStarted,
            );
            event = event.with_title(format!("Codex ({model})"));
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd);
            }
            event = event.with_occurred_at(occurred_at);
            Some(event)
        }
        "event_msg" => {
            let payload = value.get("payload")?;
            let event_type = payload.get("type")?.as_str()?;
            if event_type == "task_started" || event_type == "task_completed" {
                let session_id = session_id_from_path(path);
                let kind = if event_type == "task_completed" {
                    SessionEventKind::SessionEnded
                } else {
                    SessionEventKind::SessionUpdated
                };
                let mut event = SessionEvent::new(
                    SourceKind::Codex,
                    SessionId::new(&session_id),
                    kind,
                );
                event = event.with_occurred_at(occurred_at);
                Some(event)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Parse a Copilot JSONL line.
///
/// Copilot has types: assistant.message, assistant.turn_start, tool.execution_start, etc.
/// We generate:
/// - `assistant.turn_start` → SessionUpdated
/// - `assistant.message` with `ask_user` tool → QuestionRequested
/// - `assistant.message` (other) → SessionUpdated
fn parse_copilot_line(value: &Value, path: &Path) -> Option<SessionEvent> {
    let line_type = value.get("type")?.as_str()?;
    let occurred_at = extract_timestamp(value);

    match line_type {
        "assistant.message" => {
            let session_id = session_id_from_copilot_path(path);
            let data = value.get("data")?;

            // Check for ask_user tool calls → QuestionRequested
            if let Some(tool_requests) = data.get("toolRequests").and_then(|v| v.as_array()) {
                for tr in tool_requests {
                    if tr.get("name").and_then(|n| n.as_str()) == Some("ask_user") {
                        let args = tr.get("arguments").unwrap_or(&Value::Null);
                        let question = args
                            .get("question")
                            .and_then(|q| q.as_str())
                            .unwrap_or("Question")
                            .to_string();
                        let choices: Vec<String> = args
                            .get("choices")
                            .and_then(|c| c.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let request_id = tr
                            .get("toolCallId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("copilot-ask");

                        let event = SessionEvent::new(
                            SourceKind::Unknown,
                            SessionId::new(&session_id),
                            SessionEventKind::QuestionRequested,
                        )
                        .with_title("Copilot Chat")
                        .with_summary(question.clone())
                        .with_payload(SessionEventPayload::QuestionRequest {
                            request_id: RequestId::new(request_id),
                            prompt: question,
                            options: choices,
                        })
                        .with_occurred_at(occurred_at);

                        return Some(event);
                    }
                }
            }

            // Regular assistant message → SessionUpdated
            let mut event = SessionEvent::new(
                SourceKind::Unknown,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            );
            event = event.with_title("Copilot Chat");
            event = event.with_occurred_at(occurred_at);
            if let Some(model) = data.get("model").and_then(|m| m.as_str()) {
                event = event.with_summary(format!("model: {model}"));
            }
            Some(event)
        }
        "assistant.turn_start" => {
            let session_id = session_id_from_copilot_path(path);
            let event = SessionEvent::new(
                SourceKind::Unknown,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            )
            .with_title("Copilot Chat")
            .with_occurred_at(occurred_at);
            Some(event)
        }
        _ => None,
    }
}

// MARK: - Helpers

fn session_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn session_id_from_copilot_path(path: &Path) -> String {
    // ~/.copilot/session-state/<session-id>/events.jsonl → session-id
    path.parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn extract_timestamp(value: &Value) -> u64 {
    if let Some(ts) = value.get("timestamp") {
        if let Some(n) = ts.as_u64() {
            // If > 1e12, assume milliseconds
            return if n > 1_000_000_000_000 { n / 1000 } else { n };
        }
        if let Some(s) = ts.as_str() {
            // Try ISO 8601 — just extract epoch from known format
            return parse_iso8601_epoch(s);
        }
    }
    current_epoch_secs()
}

fn extract_assistant_summary(value: &Value) -> Option<String> {
    let content = value.get("message")?.get("content")?;
    if let Some(text) = content.as_str() {
        let truncated: String = text.chars().take(120).collect();
        return Some(truncated);
    }
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    let truncated: String = text.chars().take(120).collect();
                    return Some(truncated);
                }
            }
        }
    }
    None
}

fn parse_iso8601_epoch(s: &str) -> u64 {
    // Simple parser for "2026-03-20T07:34:51.922Z" format
    // Falls back to current time if parsing fails
    let s = s.trim().trim_end_matches('Z');
    let parts: Vec<&str> = s.split('T').collect();
    if parts.len() != 2 {
        return current_epoch_secs();
    }
    let date_parts: Vec<u64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
    if date_parts.len() != 3 {
        return current_epoch_secs();
    }
    let time_str = parts[1].split('.').next().unwrap_or("00:00:00");
    let time_parts: Vec<u64> = time_str.split(':').filter_map(|p| p.parse().ok()).collect();
    if time_parts.len() != 3 {
        return current_epoch_secs();
    }

    // Rough epoch calculation (ignoring leap years for simplicity)
    let year = date_parts[0];
    let month = date_parts[1];
    let day = date_parts[2];
    let days_since_epoch = (year - 1970) * 365 + (year - 1969) / 4
        + days_before_month(month, is_leap(year))
        + day
        - 1;
    days_since_epoch * 86400 + time_parts[0] * 3600 + time_parts[1] * 60 + time_parts[2]
}

fn days_before_month(month: u64, leap: bool) -> u64 {
    const DAYS: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let m = (month as usize).saturating_sub(1).min(11);
    DAYS[m] + if leap && month > 2 { 1 } else { 0 }
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn is_recently_modified(path: &Path, now_secs: u64) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| now_secs.saturating_sub(d.as_secs()) < MAX_FILE_AGE_SECS)
        .unwrap_or(false)
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn parse_claude_user_line_produces_session_updated() {
        let line = r#"{"type":"user","sessionId":"sess-1","cwd":"/tmp","slug":"fix-bug","timestamp":1700000000,"message":{"role":"user","content":"hello"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_claude_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
        assert_eq!(event.session_id.as_str(), "sess-1");
        assert_eq!(event.title.as_deref(), Some("fix bug"));
        assert_eq!(event.cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn parse_claude_exit_produces_session_ended() {
        let line = r#"{"type":"user","sessionId":"sess-1","timestamp":1700000000,"message":{"role":"user","content":"<command-name>/exit</command-name>"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_claude_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionEnded);
    }

    #[test]
    fn parse_claude_assistant_line_extracts_summary() {
        let line = r#"{"type":"assistant","sessionId":"sess-1","timestamp":1700000000,"message":{"role":"assistant","content":"I'll help you fix the bug."}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_claude_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
        assert!(event.summary.unwrap().contains("fix the bug"));
    }

    #[test]
    fn parse_claude_system_line_returns_none() {
        let line = r#"{"type":"system","sessionId":"sess-1","subtype":"turn_duration","timestamp":1700000000}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        assert!(parse_claude_line(&value, Path::new("test.jsonl")).is_none());
    }

    #[test]
    fn parse_codex_session_meta_produces_session_started() {
        let line = r#"{"type":"session_meta","timestamp":"2026-03-20T07:34:51.922Z","payload":{"id":"codex-sess-1","cwd":"/tmp/project","model_provider":"openai"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_codex_line(&value, Path::new("test.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        assert_eq!(event.session_id.as_str(), "codex-sess-1");
        assert_eq!(event.cwd.as_deref(), Some("/tmp/project"));
        assert!(event.title.unwrap().contains("Codex"));
    }

    #[test]
    fn parse_codex_task_started_produces_session_updated() {
        let line = r#"{"type":"event_msg","timestamp":"2026-03-20T07:35:00.000Z","payload":{"type":"task_started","turn_id":"turn-1"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_codex_line(&value, Path::new("codex-sess-1.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
    }

    #[test]
    fn parse_codex_task_completed_produces_session_ended() {
        let line = r#"{"type":"event_msg","timestamp":"2026-03-20T08:00:00.000Z","payload":{"type":"task_completed"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let event = parse_codex_line(&value, Path::new("codex-sess-1.jsonl")).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionEnded);
    }

    #[test]
    fn parse_copilot_turn_start_produces_session_updated() {
        let line = r#"{"type":"assistant.turn_start","timestamp":"2026-03-20T07:35:00.000Z","data":{}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/home/user/.copilot/session-state/abc-123/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionUpdated);
        assert_eq!(event.session_id.as_str(), "abc-123");
        assert_eq!(event.title.as_deref(), Some("Copilot Chat"));
    }

    #[test]
    fn parse_copilot_assistant_message_extracts_model() {
        let line = r#"{"type":"assistant.message","timestamp":"2026-03-20T07:35:00.000Z","data":{"model":"claude-sonnet-4.6"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/copilot/session-state/sess-x/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert!(event.summary.unwrap().contains("claude-sonnet-4.6"));
    }

    #[test]
    fn parse_copilot_ask_user_produces_question_requested() {
        let line = r#"{"type":"assistant.message","timestamp":"2026-03-20T07:35:00.000Z","data":{"toolRequests":[{"name":"ask_user","toolCallId":"call-123","arguments":{"question":"Which database?","choices":["PostgreSQL","MySQL","SQLite"]}}]}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/copilot/session-state/sess-ask/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::QuestionRequested);
        assert_eq!(event.session_id.as_str(), "sess-ask");
        if let SessionEventPayload::QuestionRequest {
            request_id,
            prompt,
            options,
        } = &event.payload
        {
            assert_eq!(request_id.as_str(), "call-123");
            assert_eq!(prompt, "Which database?");
            assert_eq!(options, &["PostgreSQL", "MySQL", "SQLite"]);
        } else {
            panic!("expected QuestionRequest payload");
        }
    }

    #[test]
    fn session_id_from_path_extracts_stem() {
        let path = Path::new("/home/.claude/projects/abc/7dd34bc1.jsonl");
        assert_eq!(session_id_from_path(path), "7dd34bc1");
    }

    #[test]
    fn session_id_from_copilot_path_extracts_parent_dir() {
        let path = Path::new("/home/.copilot/session-state/my-session-id/events.jsonl");
        assert_eq!(session_id_from_copilot_path(path), "my-session-id");
    }

    #[test]
    fn read_new_lines_tracks_cursor_position() {
        let dir = std::env::temp_dir().join(format!("pupkit-watcher-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test.jsonl");

        // Write initial content
        {
            let mut f = File::create(&file_path).unwrap();
            writeln!(f, r#"{{"type":"user","line":1}}"#).unwrap();
            writeln!(f, r#"{{"type":"user","line":2}}"#).unwrap();
        }

        let mut cursors = FileCursors::default();

        // First read: gets both lines
        let lines = read_new_lines(&file_path, &mut cursors);
        assert_eq!(lines.len(), 2);

        // Second read: no new lines
        let lines = read_new_lines(&file_path, &mut cursors);
        assert_eq!(lines.len(), 0);

        // Append more content
        {
            let mut f = fs::OpenOptions::new().append(true).open(&file_path).unwrap();
            writeln!(f, r#"{{"type":"assistant","line":3}}"#).unwrap();
        }

        // Third read: gets only the new line
        let lines = read_new_lines(&file_path, &mut cursors);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"line\":3"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_timestamp_handles_integer_and_iso8601() {
        let v: Value = serde_json::from_str(r#"{"timestamp":1700000000}"#).unwrap();
        assert_eq!(extract_timestamp(&v), 1700000000);

        let v: Value = serde_json::from_str(r#"{"timestamp":1700000000000}"#).unwrap();
        assert_eq!(extract_timestamp(&v), 1700000000); // milliseconds → seconds

        // ISO 8601 should produce a reasonable epoch
        let v: Value = serde_json::from_str(r#"{"timestamp":"2026-01-01T00:00:00.000Z"}"#).unwrap();
        let ts = extract_timestamp(&v);
        assert!(ts > 1_700_000_000 && ts < 2_000_000_000);
    }
}
