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

// MARK: - Pending tool call tracker (Copilot approval detection)

/// Tools that typically require user approval in Copilot CLI.
const APPROVAL_TOOLS: &[&str] = &[
    "bash", "edit", "create", "write_bash", "stop_bash",
];

/// Tracks tool calls from `assistant.message` that haven't been started yet.
/// If a tool call persists across two poll cycles, we emit an ApprovalRequested event.
#[derive(Default)]
struct PendingToolTracker {
    /// tool_call_id → (session_id, tool_name, command_summary, first_seen_poll)
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
        // Collect IDs that have been pending for at least 1 full poll cycle
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
                                eprintln!("[watcher] TTY discovered for {}: {}", event.session_id.as_str(), tty.display());
                                daemon.copilot_ttys_mut().set(
                                    event.session_id.clone(),
                                    tty,
                                    options.clone(),
                                    SourceKind::Copilot,
                                );
                            }
                            None => {
                                eprintln!("[watcher] TTY discovery failed for session dir: {}", session_dir.display());
                            }
                        }
                    }
                }
                // For ApprovalRequested from watcher, also register TTY for approve/deny injection
                if event.kind == SessionEventKind::ApprovalRequested {
                    let copilot_root = home.join(".copilot/session-state");
                    let session_dir = copilot_root.join(event.session_id.as_str());
                    if let Some(tty) = tty_inject::discover_tty(&session_dir) {
                        eprintln!("[watcher] TTY for approval: {}: {}", event.session_id.as_str(), tty.display());
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
                eprintln!("[watcher] approval needed: {} ({}) in session {} [{:?}]", tool_name, summary, session_id, source);
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
                                eprintln!("[watcher] TTY for {:?} approval: {}: {}", source, session_id, tty.display());
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
            // Always use filename-based session_id so all events from the same
            // file share the same ID (event_msg and track_codex_tool_calls
            // already use session_id_from_path).
            let session_id = session_id_from_path(path);
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
/// Copilot has types: session.start, assistant.message, assistant.turn_start,
/// tool.execution_start, etc.
/// We generate:
/// - `session.start` → SessionStarted (with CWD)
/// - `assistant.turn_start` → SessionUpdated
/// - `assistant.message` with `ask_user` tool → QuestionRequested
/// - `assistant.message` (other) → SessionUpdated
fn parse_copilot_line(value: &Value, path: &Path) -> Option<SessionEvent> {
    let line_type = value.get("type")?.as_str()?;
    let occurred_at = extract_timestamp(value);

    match line_type {
        "session.start" => {
            let session_id = session_id_from_copilot_path(path);
            let data = value.get("data")?;
            let cwd = data
                .get("context")
                .and_then(|c| c.get("cwd"))
                .and_then(|v| v.as_str());
            let title = cwd
                .and_then(|c| c.rsplit('/').next())
                .map(|dir| format!("Copilot · {dir}"))
                .unwrap_or_else(|| "Copilot Chat".to_string());
            let mut event = SessionEvent::new(
                SourceKind::Copilot,
                SessionId::new(&session_id),
                SessionEventKind::SessionStarted,
            )
            .with_title(title)
            .with_occurred_at(occurred_at);
            if let Some(cwd) = cwd {
                event = event.with_cwd(cwd.to_string());
            }
            Some(event)
        }
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
                            SourceKind::Copilot,
                            SessionId::new(&session_id),
                            SessionEventKind::QuestionRequested,
                        )
                        .with_summary(question.clone())
                        .with_payload(SessionEventPayload::QuestionRequest {
                            request_id: RequestId::new(request_id),
                            prompt: question,
                            options: choices,
                            allow_freeform: true,
                        })
                        .with_occurred_at(occurred_at);

                        return Some(event);
                    }
                }
            }

            // Regular assistant message → SessionUpdated
            let mut event = SessionEvent::new(
                SourceKind::Copilot,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            );
            event = event.with_occurred_at(occurred_at);
            if let Some(model) = data.get("model").and_then(|m| m.as_str()) {
                event = event.with_summary(format!("model: {model}"));
            }
            Some(event)
        }
        "assistant.turn_start" => {
            let session_id = session_id_from_copilot_path(path);
            let event = SessionEvent::new(
                SourceKind::Copilot,
                SessionId::new(&session_id),
                SessionEventKind::SessionUpdated,
            )
            .with_occurred_at(occurred_at);
            Some(event)
        }
        _ => None,
    }
}

/// Track Copilot tool calls for approval detection.
///
/// Records tool requests from `assistant.message` for approval-needing tools,
/// and marks them as started when `tool.execution_start` is seen.
fn track_copilot_tool_calls(value: &Value, path: &Path, tracker: &mut PendingToolTracker) {
    let line_type = match value.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return,
    };

    let session_id = session_id_from_copilot_path(path);

    match line_type {
        "assistant.message" => {
            let data = match value.get("data") {
                Some(d) => d,
                None => return,
            };
            if let Some(trs) = data.get("toolRequests").and_then(|v| v.as_array()) {
                for tr in trs {
                    let tool_name = match tr.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n,
                        None => continue,
                    };
                    if !APPROVAL_TOOLS.contains(&tool_name) {
                        continue;
                    }
                    let tool_call_id = match tr.get("toolCallId").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => continue,
                    };
                    let summary = tr
                        .get("arguments")
                        .and_then(|a| {
                            a.get("command")
                                .or_else(|| a.get("path"))
                                .or_else(|| a.get("file_text"))
                        })
                        .and_then(|v| v.as_str())
                        .map(|s| truncate_summary(s))
                        .unwrap_or_else(|| tool_name.to_string());

                    tracker.record_requested(
                        tool_call_id,
                        session_id.clone(),
                        SourceKind::Copilot,
                        tool_name.to_string(),
                        summary,
                        None,
                    );
                }
            }
        }
        "tool.execution_start" => {
            if let Some(tool_call_id) = value
                .get("data")
                .and_then(|d| d.get("toolCallId"))
                .and_then(|v| v.as_str())
            {
                tracker.mark_started(tool_call_id);
            }
        }
        _ => {}
    }
}

/// Approval-relevant tool names for Claude Code.
const CLAUDE_APPROVAL_TOOLS: &[&str] = &["Bash", "Write", "Edit", "MultiEdit"];

/// Track Claude Code tool calls for approval detection.
///
/// Records `tool_use` from assistant messages and clears them on `tool_result`.
fn track_claude_tool_calls(value: &Value, path: &Path, tracker: &mut PendingToolTracker) {
    let line_type = match value.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return,
    };
    let session_id = value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| session_id_from_path(path));

    match line_type {
        "assistant" => {
            let message = match value.get("message") {
                Some(m) => m,
                None => return,
            };
            // Only track when stop_reason is "tool_use" (model wants to call a tool)
            if message.get("stop_reason").and_then(|v| v.as_str()) != Some("tool_use") {
                return;
            }
            if let Some(contents) = message.get("content").and_then(|c| c.as_array()) {
                for item in contents {
                    if item.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                        continue;
                    }
                    let tool_name = match item.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n,
                        None => continue,
                    };
                    if !CLAUDE_APPROVAL_TOOLS.contains(&tool_name) {
                        continue;
                    }
                    let tool_call_id = match item.get("id").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => continue,
                    };
                    let summary = item
                        .get("input")
                        .and_then(|inp| {
                            inp.get("command")
                                .or_else(|| inp.get("path"))
                                .or_else(|| inp.get("file_path"))
                        })
                        .and_then(|v| v.as_str())
                        .map(|s| truncate_summary(s))
                        .unwrap_or_else(|| tool_name.to_string());

                    tracker.record_requested(
                        tool_call_id,
                        session_id.clone(),
                        SourceKind::ClaudeCode,
                        tool_name.to_string(),
                        summary,
                        Some(path.to_path_buf()),
                    );
                }
            }
        }
        "user" => {
            // tool_result clears pending tool calls
            if let Some(contents) = value
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in contents {
                    if item.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        if let Some(id) = item.get("tool_use_id").and_then(|v| v.as_str()) {
                            tracker.mark_started(id);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Codex approval-relevant tool names.
const CODEX_APPROVAL_TOOLS: &[&str] = &["exec_command", "apply_patch"];

/// Track Codex tool calls for approval detection.
///
/// Records `function_call` from response_item and clears on `function_call_output`
/// or `exec_command_end`.
fn track_codex_tool_calls(value: &Value, path: &Path, tracker: &mut PendingToolTracker) {
    let line_type = match value.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => return,
    };
    let session_id = session_id_from_path(path);

    match line_type {
        "response_item" => {
            let payload = match value.get("payload") {
                Some(p) => p,
                None => return,
            };
            let payload_type = match payload.get("type").and_then(|t| t.as_str()) {
                Some(t) => t,
                None => return,
            };
            match payload_type {
                "function_call" => {
                    let tool_name = match payload.get("name").and_then(|n| n.as_str()) {
                        Some(n) => n,
                        None => return,
                    };
                    if !CODEX_APPROVAL_TOOLS.contains(&tool_name) {
                        return;
                    }
                    let call_id = match payload.get("call_id").and_then(|v| v.as_str()) {
                        Some(id) => id.to_string(),
                        None => return,
                    };
                    let summary = payload
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        .and_then(|args| {
                            args.get("cmd")
                                .or_else(|| args.get("command"))
                                .and_then(|v| v.as_str())
                                .map(|s| truncate_summary(s))
                        })
                        .unwrap_or_else(|| tool_name.to_string());

                    tracker.record_requested(
                        call_id,
                        session_id,
                        SourceKind::Codex,
                        tool_name.to_string(),
                        summary,
                        Some(path.to_path_buf()),
                    );
                }
                "function_call_output" => {
                    if let Some(call_id) = payload.get("call_id").and_then(|v| v.as_str()) {
                        tracker.mark_started(call_id);
                    }
                }
                "custom_tool_call" => {
                    // apply_patch with status "completed" — clear immediately
                    if payload.get("status").and_then(|v| v.as_str()) == Some("completed") {
                        if let Some(call_id) = payload.get("call_id").and_then(|v| v.as_str()) {
                            tracker.mark_started(call_id);
                        }
                    }
                }
                _ => {}
            }
        }
        "event_msg" => {
            // exec_command_end also clears pending
            if let Some(payload) = value.get("payload") {
                if payload.get("type").and_then(|t| t.as_str()) == Some("exec_command_end") {
                    if let Some(call_id) = payload.get("call_id").and_then(|v| v.as_str()) {
                        tracker.mark_started(call_id);
                    }
                }
            }
        }
        _ => {}
    }
}

fn truncate_summary(s: &str) -> String {
    if s.len() > 80 {
        format!("{}…", &s[..77])
    } else {
        s.to_string()
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
        // session_id comes from the filename, not payload.id
        assert_eq!(event.session_id.as_str(), "test");
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
        // turn_start no longer sets a title — the CWD-derived title from
        // session.start persists via the registry.
        assert_eq!(event.title, None);
    }

    #[test]
    fn parse_copilot_session_start_extracts_cwd_and_title() {
        let line = r#"{"type":"session.start","timestamp":"2026-04-01T02:57:21.058Z","data":{"sessionId":"c807f80f","copilotVersion":"1.0.14","startTime":"2026-04-01T02:57:21.032Z","context":{"cwd":"/Users/dev/projects/my-app"}}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/home/.copilot/session-state/c807f80f/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        assert_eq!(event.session_id.as_str(), "c807f80f");
        assert_eq!(event.title.as_deref(), Some("Copilot · my-app"));
        assert_eq!(event.cwd.as_deref(), Some("/Users/dev/projects/my-app"));
    }

    #[test]
    fn parse_copilot_session_start_without_cwd_uses_fallback_title() {
        let line = r#"{"type":"session.start","timestamp":"2026-04-01T02:57:21.058Z","data":{"sessionId":"abc"}}"#;
        let value: Value = serde_json::from_str(line).unwrap();
        let path = Path::new("/home/.copilot/session-state/abc/events.jsonl");
        let event = parse_copilot_line(&value, path).unwrap();
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        assert_eq!(event.title.as_deref(), Some("Copilot Chat"));
        assert_eq!(event.cwd, None);
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
            allow_freeform,
        } = &event.payload
        {
            assert_eq!(request_id.as_str(), "call-123");
            assert_eq!(prompt, "Which database?");
            assert_eq!(options, &["PostgreSQL", "MySQL", "SQLite"]);
            assert!(*allow_freeform);
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

    #[test]
    fn copilot_session_start_emits_started_with_cwd() {
        let json = r#"{
            "type": "session.start",
            "data": {
                "sessionId": "abc-123",
                "copilotVersion": "1.0.14",
                "context": { "cwd": "/Users/test/my-project" }
            },
            "timestamp": "2026-04-01T02:57:21.058Z"
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        let path = Path::new("/home/.copilot/session-state/abc-123/events.jsonl");
        let event = parse_copilot_line(&value, path).expect("should parse session.start");
        assert_eq!(event.kind, SessionEventKind::SessionStarted);
        assert_eq!(event.source, SourceKind::Copilot);
        assert_eq!(event.cwd.as_deref(), Some("/Users/test/my-project"));
        assert_eq!(event.title.as_deref(), Some("Copilot · my-project"));
    }

    #[test]
    fn copilot_source_kind_is_copilot_not_unknown() {
        let json = r#"{
            "type": "assistant.turn_start",
            "data": {},
            "timestamp": "2026-04-01T03:00:00.000Z"
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        let path = Path::new("/home/.copilot/session-state/sess-1/events.jsonl");
        let event = parse_copilot_line(&value, path).expect("should parse turn_start");
        assert_eq!(event.source, SourceKind::Copilot);
    }

    // --- Claude Code approval tracking tests ---

    #[test]
    fn claude_tool_use_records_pending_and_tool_result_clears() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/session-1.jsonl");

        // assistant with tool_use
        let tool_use_json = r#"{
            "type": "assistant",
            "sessionId": "session-1",
            "message": {
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "call_abc",
                    "name": "Bash",
                    "input": { "command": "ls -la" }
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(tool_use_json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);
        assert!(tracker.pending.contains_key("call_abc"));
        assert_eq!(tracker.pending["call_abc"].source, SourceKind::ClaudeCode);

        // user with tool_result clears it
        let result_json = r#"{
            "type": "user",
            "sessionId": "session-1",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": "call_abc",
                    "content": "file1.rs\nfile2.rs"
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(result_json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn claude_non_approval_tool_is_ignored() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/session-1.jsonl");

        let json = r#"{
            "type": "assistant",
            "sessionId": "session-1",
            "message": {
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "call_xyz",
                    "name": "Read",
                    "input": { "path": "/etc/hosts" }
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        // "Read" is not in CLAUDE_APPROVAL_TOOLS
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn claude_end_turn_is_not_tracked() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/session-1.jsonl");

        let json = r#"{
            "type": "assistant",
            "sessionId": "session-1",
            "message": {
                "stop_reason": "end_turn",
                "content": [{ "type": "text", "text": "Done!" }]
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn claude_stale_tool_call_becomes_approval() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.claude/projects/abc/sess.jsonl");

        let json = r#"{
            "type": "assistant",
            "sessionId": "sess",
            "message": {
                "stop_reason": "tool_use",
                "content": [{
                    "type": "tool_use",
                    "id": "call_stale",
                    "name": "Bash",
                    "input": { "command": "rm -rf /" }
                }]
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_claude_tool_calls(&value, path, &mut tracker);

        // First advance: tool just seen, not yet stale
        let approvals = tracker.advance_poll();
        assert!(approvals.is_empty());

        // Second advance: now it's stale → approval emitted
        let approvals = tracker.advance_poll();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].0, "sess"); // session_id
        assert_eq!(approvals[0].1, SourceKind::ClaudeCode); // source
        assert_eq!(approvals[0].2, "Bash"); // tool_name
    }

    // --- Codex approval tracking tests ---

    #[test]
    fn codex_function_call_records_pending_and_output_clears() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/2026/04/08/rollout-sess1.jsonl");

        let call_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "call_id": "call_codex_1",
                "arguments": "{\"cmd\":\"pwd\"}"
            }
        }"#;
        let value: Value = serde_json::from_str(call_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);
        assert!(tracker.pending.contains_key("call_codex_1"));
        assert_eq!(tracker.pending["call_codex_1"].source, SourceKind::Codex);

        // function_call_output clears it
        let output_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call_output",
                "call_id": "call_codex_1",
                "output": "/Users/test"
            }
        }"#;
        let value: Value = serde_json::from_str(output_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn codex_exec_command_end_also_clears_pending() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/rollout-sess2.jsonl");

        // Record a pending call
        let call_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "call_id": "call_end_test",
                "arguments": "{\"cmd\":\"echo hi\"}"
            }
        }"#;
        let value: Value = serde_json::from_str(call_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);

        // exec_command_end clears it
        let end_json = r#"{
            "type": "event_msg",
            "payload": {
                "type": "exec_command_end",
                "call_id": "call_end_test",
                "exit_code": 0,
                "status": "completed"
            }
        }"#;
        let value: Value = serde_json::from_str(end_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }

    #[test]
    fn codex_stale_function_call_becomes_approval() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/rollout-sess3.jsonl");

        let json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "exec_command",
                "call_id": "call_stale_codex",
                "arguments": "{\"cmd\":\"dangerous-cmd\"}"
            }
        }"#;
        let value: Value = serde_json::from_str(json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);

        let _ = tracker.advance_poll(); // not stale yet
        let approvals = tracker.advance_poll(); // now stale
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].1, SourceKind::Codex);
        assert_eq!(approvals[0].2, "exec_command");
    }

    #[test]
    fn codex_completed_custom_tool_call_is_auto_cleared() {
        let mut tracker = PendingToolTracker::default();
        let path = Path::new("/home/.codex/sessions/rollout-sess4.jsonl");

        // First record a function_call for apply_patch
        let call_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "name": "apply_patch",
                "call_id": "call_patch_1",
                "arguments": "{}"
            }
        }"#;
        let value: Value = serde_json::from_str(call_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 1);

        // custom_tool_call with status "completed" clears it
        let completed_json = r#"{
            "type": "response_item",
            "payload": {
                "type": "custom_tool_call",
                "status": "completed",
                "call_id": "call_patch_1",
                "name": "apply_patch"
            }
        }"#;
        let value: Value = serde_json::from_str(completed_json).unwrap();
        track_codex_tool_calls(&value, path, &mut tracker);
        assert_eq!(tracker.pending.len(), 0);
    }
}
