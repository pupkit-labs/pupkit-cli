mod claude;
mod codex;
mod copilot;

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::{log_debug, log_info, log_warn};

use crate::daemon::PupkitDaemon;
use crate::daemon::tty_inject;
use crate::protocol::{
    RequestId, SessionEvent, SessionEventKind, SessionEventPayload, SessionId, SourceKind,
};

use claude::{parse_claude_line, track_claude_tool_calls};
use codex::{parse_codex_line, track_codex_tool_calls};
use copilot::{parse_copilot_line, track_copilot_tool_calls};

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

// MARK: - Pending tool call tracker (approval detection)

/// Tools that typically require user approval in Copilot CLI.
const APPROVAL_TOOLS: &[&str] = &[
    "bash", "edit", "create", "write_bash", "stop_bash",
];

/// Tracks tool calls from `assistant.message` that haven't been started yet.
/// If a tool call persists across two poll cycles, we emit an ApprovalRequested event.
#[derive(Default)]
struct PendingToolTracker {
    /// tool_call_id → PendingToolCall
    pending: HashMap<String, PendingToolCall>,
    poll_count: u64,
}

struct PendingToolCall {
    session_id: String,
    source: SourceKind,
    tool_name: String,
    summary: String,
    first_seen_poll: u64,
    jsonl_path: Option<PathBuf>,
}

impl PendingToolTracker {
    /// Record tool calls from an assistant.message event.
    fn record_requested(&mut self, tool_call_id: String, session_id: String, source: SourceKind, tool_name: String, summary: String, jsonl_path: Option<PathBuf>) {
        self.pending.entry(tool_call_id).or_insert(PendingToolCall {
            session_id,
            source,
            tool_name,
            summary,
            first_seen_poll: self.poll_count,
            jsonl_path,
        });
    }

    /// Mark a tool call as started (removes from pending).
    fn mark_started(&mut self, tool_call_id: &str) {
        self.pending.remove(tool_call_id);
    }

    /// Advance the poll counter and return approval events for tool calls
    /// that have been pending since the previous poll (survived one full cycle).
    /// Returns (session_id, source, tool_name, summary, jsonl_path) tuples.
    fn advance_poll(&mut self) -> Vec<(String, SourceKind, String, String, Option<PathBuf>)> {
        self.poll_count += 1;
        let cutoff = self.poll_count.saturating_sub(1);
        let mut approvals = Vec::new();
        let stale_ids: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, tc)| tc.first_seen_poll < cutoff)
            .map(|(id, _)| id.clone())
            .collect();
        for id in stale_ids {
            if let Some(tc) = self.pending.remove(&id) {
                approvals.push((tc.session_id, tc.source, tc.tool_name, tc.summary, tc.jsonl_path));
            }
        }
        approvals
    }

    /// Remove all pending entries for a session (e.g., when execution starts).
    #[allow(dead_code)]
    fn clear_session(&mut self, session_id: &str) {
        self.pending.retain(|_, tc| tc.session_id != session_id);
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
    let mut tool_tracker = PendingToolTracker::default();

    // Seed session metadata (CWD, title) from first lines before seeking to end.
    let seed_events = seed_sessions_from_first_lines(&sources);

    // On first run, seek all existing files to end so we only see new events.
    initialize_cursors(&sources, &mut cursors);

    // Apply seed events so sessions have proper titles on restart.
    if !seed_events.is_empty() {
        if let Ok(mut daemon) = daemon.lock() {
            for event in seed_events {
                let _ = daemon.ingest_event(event);
            }
        }
    }

    loop {
        thread::sleep(POLL_INTERVAL);

        let events = poll_all_sources(&sources, &mut cursors, &mut tool_tracker);

        // Check for tool calls that have been pending for >1 poll cycle → approval needed
        let approvals = tool_tracker.advance_poll();

        if events.is_empty() && approvals.is_empty() {
            continue;
        }

        if let Ok(mut daemon) = daemon.lock() {
            for event in events {
                // For Copilot QuestionRequested events, discover and register the TTY
                if event.kind == SessionEventKind::QuestionRequested {
                    if let SessionEventPayload::QuestionRequest { ref options, .. } = event.payload
                    {
                        let copilot_root = home.join(".copilot/session-state");
                        let session_dir = copilot_root.join(event.session_id.as_str());
                        match tty_inject::discover_tty(&session_dir) {
                            Some(tty) => {
                                log_debug!("[watcher] TTY discovered for {}: {}", event.session_id.as_str(), tty.display());
                                daemon.copilot_ttys_mut().set(
                                    event.session_id.clone(),
                                    tty,
                                    options.clone(),
                                    SourceKind::Copilot,
                                );
                            }
                            None => {
                                log_warn!("[watcher] TTY discovery failed for session dir: {}", session_dir.display());
                            }
                        }
                    }
                }
                // For ApprovalRequested from watcher, also register TTY for approve/deny injection
                if event.kind == SessionEventKind::ApprovalRequested {
                    let copilot_root = home.join(".copilot/session-state");
                    let session_dir = copilot_root.join(event.session_id.as_str());
                    if let Some(tty) = tty_inject::discover_tty(&session_dir) {
                        log_debug!("[watcher] TTY for approval: {}: {}", event.session_id.as_str(), tty.display());
                        daemon.copilot_ttys_mut().set(
                            event.session_id.clone(),
                            tty,
                            vec!["allow".into(), "deny".into()],
                            SourceKind::Copilot,
                        );
                    }
                }
                let _ = daemon.ingest_event(event);
            }

            // Emit ApprovalRequested events for stale pending tool calls
            for (session_id, source, tool_name, summary, jsonl_path) in approvals {
                log_info!("[watcher] approval needed: {} ({}) in session {} [{:?}]", tool_name, summary, session_id, source);
                let title = match &source {
                    SourceKind::ClaudeCode => "Claude Code",
                    SourceKind::Codex => "Codex",
                    SourceKind::Copilot => "Copilot Chat",
                    SourceKind::Unknown => "AI Tool",
                };
                let event = SessionEvent::new(
                    source.clone(),
                    SessionId::new(&session_id),
                    SessionEventKind::ApprovalRequested,
                )
                .with_title(title)
                .with_summary(format!("{tool_name}: {summary}"))
                .with_payload(SessionEventPayload::ApprovalRequest {
                    request_id: RequestId::new(format!("approve-{}", current_epoch_secs())),
                    tool_name,
                    tool_input_summary: summary,
                });
                // Discover TTY for approval injection
                match &source {
                    SourceKind::Copilot => {
                        let copilot_root = home.join(".copilot/session-state");
                        let session_dir = copilot_root.join(&session_id);
                        if let Some(tty) = tty_inject::discover_tty(&session_dir) {
                            daemon.copilot_ttys_mut().set(
                                SessionId::new(&session_id),
                                tty,
                                vec!["allow".into(), "deny".into()],
                                SourceKind::Copilot,
                            );
                        }
                    }
                    SourceKind::ClaudeCode | SourceKind::Codex => {
                        if let Some(path) = &jsonl_path {
                            if let Some(tty) = tty_inject::discover_tty_from_jsonl(path) {
                                log_debug!("[watcher] TTY for {:?} approval: {}: {}", source, session_id, tty.display());
                                // Claude Code TUI: Yes / Yes-all / No (3 options)
                                daemon.copilot_ttys_mut().set(
                                    SessionId::new(&session_id),
                                    tty,
                                    vec!["allow".into(), "allow_always".into(), "deny".into()],
                                    source.clone(),
                                );
                            }
                        }
                    }
                    _ => {}
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

/// Seed session metadata by reading the first line of each recently-modified
/// JSONL file. This ensures `session.start` (Copilot) and `session_meta`
/// (Codex) events set the CWD-derived title even when the daemon restarts
/// and `initialize_cursors` seeks past them.
fn seed_sessions_from_first_lines(sources: &[WatchSource]) -> Vec<SessionEvent> {
    let mut events = Vec::new();
    let now = current_epoch_secs();
    for source in sources {
        let files = find_jsonl_files(&source.root, &source.kind);
        for path in files {
            if !is_recently_modified(&path, now) {
                continue;
            }
            if let Ok(file) = File::open(&path) {
                let reader = BufReader::new(file);
                if let Some(Ok(first_line)) = reader.lines().next() {
                    if let Some(event) = parse_line(&first_line, &source.kind, &path) {
                        events.push(event);
                    }
                }
            }
        }
    }
    events
}

fn poll_all_sources(
    sources: &[WatchSource],
    cursors: &mut FileCursors,
    tool_tracker: &mut PendingToolTracker,
) -> Vec<SessionEvent> {
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
                // Track tool calls for approval detection (all sources)
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    match source.kind {
                        WatchSourceKind::Copilot => track_copilot_tool_calls(&value, &path, tool_tracker),
                        WatchSourceKind::Claude => track_claude_tool_calls(&value, &path, tool_tracker),
                        WatchSourceKind::Codex => track_codex_tool_calls(&value, &path, tool_tracker),
                    }
                }
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

// MARK: - Helpers

fn truncate_summary(s: &str) -> String {
    if s.len() > 80 {
        format!("{}…", &s[..77])
    } else {
        s.to_string()
    }
}

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
