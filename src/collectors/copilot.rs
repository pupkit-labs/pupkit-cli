use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::model::{CopilotQuotaEntry, CopilotQuotaInfo, CopilotUsageSummary, UsageAvailability};

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const HINT: &str = "Run /usage in Copilot CLI for quota details";
const DAY_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const EVENT_TURN_START: &str = "assistant.turn_start";
const EVENT_MESSAGE: &str = "assistant.message";
const DEFAULT_API_PORT: u16 = 1414;
const API_TIMEOUT_SECS: &str = "5";
const API_CONNECT_TIMEOUT_SECS: &str = "2";

pub fn collect_copilot_usage_summary() -> CopilotUsageSummary {
    let home = env::var_os("HOME").map(PathBuf::from);
    let mut summary =
        collect_copilot_usage_summary_with_home(home.as_deref(), SystemTime::now());

    let quota = fetch_copilot_api_quota(&mut default_runner);
    if let Some(ref q) = quota {
        summary.plan_type = q.plan.clone();
        summary.availability = match summary.availability {
            UsageAvailability::Unavailable => UsageAvailability::Partial,
            other => other,
        };
    }
    summary.quota = quota;
    summary
}

fn collect_copilot_usage_summary_with_home(
    home: Option<&Path>,
    now: SystemTime,
) -> CopilotUsageSummary {
    let Some(home) = home else {
        return unavailable_summary();
    };

    let sessions_dir = home.join(".copilot/session-state");

    let mut aggregate = CopilotAggregate::default();

    if sessions_dir.is_dir() {
        scan_sessions_dir(&sessions_dir, now, &mut aggregate);
    }

    let model = if aggregate.latest_model.is_empty() {
        detect_model_from_config(home)
    } else {
        aggregate.latest_model.clone()
    };

    let plan_type = detect_plan_type(home);

    let availability = if aggregate.total_requests > 0 {
        UsageAvailability::Live
    } else if sessions_dir.is_dir() {
        UsageAvailability::Partial
    } else {
        UsageAvailability::Unavailable
    };

    CopilotUsageSummary {
        availability,
        model,
        plan_type,
        last_active_at: format_optional_timestamp(aggregate.latest_activity_at),
        total_requests: if aggregate.total_requests > 0 {
            Some(aggregate.total_requests)
        } else {
            None
        },
        last_24h_requests: if aggregate.total_requests > 0 {
            Some(aggregate.last_24h_requests)
        } else {
            None
        },
        total_sessions: if aggregate.total_sessions > 0 {
            Some(aggregate.total_sessions)
        } else {
            None
        },
        remaining_percent: None,
        hint: HINT.to_string(),
        quota: None,
    }
}

#[derive(Default)]
struct CopilotAggregate {
    total_requests: u64,
    last_24h_requests: u64,
    total_sessions: u64,
    latest_model: String,
    latest_activity_at: Option<SystemTime>,
}

fn scan_sessions_dir(dir: &Path, now: SystemTime, aggregate: &mut CopilotAggregate) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }

        let events_path = path.join("events.jsonl");
        if !events_path.exists() {
            continue;
        }

        aggregate.total_sessions += 1;
        let file_mtime = file_modified_at(&events_path);
        update_latest_time(&mut aggregate.latest_activity_at, file_mtime);

        let _ = process_events_file(&events_path, file_mtime, now, aggregate);
    }
}

fn process_events_file(
    path: &Path,
    file_mtime: Option<SystemTime>,
    now: SystemTime,
    aggregate: &mut CopilotAggregate,
) -> Option<()> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let in_last_24h = file_mtime
        .and_then(|mtime| now.duration_since(mtime).ok())
        .map(|age| age <= DAY_WINDOW)
        .unwrap_or(false);

    let mut session_model = String::new();

    for line in reader.lines().map_while(Result::ok) {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let Ok(value): Result<Value, _> = serde_json::from_str(&line) else {
            continue;
        };

        let event_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            t if t == EVENT_TURN_START => {
                aggregate.total_requests += 1;
                if in_last_24h {
                    aggregate.last_24h_requests += 1;
                }
            }
            t if t == EVENT_MESSAGE => {
                if let Some(model) = value
                    .get("data")
                    .and_then(|d| d.get("model"))
                    .and_then(|m| m.as_str())
                    .filter(|m| !m.is_empty())
                {
                    session_model = model.to_string();
                }
            }
            _ => {}
        }
    }

    if !session_model.is_empty() {
        let is_newer = match (
            &aggregate.latest_activity_at,
            file_mtime,
        ) {
            (Some(latest), Some(mtime)) => mtime >= *latest,
            (None, Some(_)) => true,
            _ => false,
        };
        if is_newer || aggregate.latest_model.is_empty() {
            aggregate.latest_model = session_model;
        }
    }

    Some(())
}

fn detect_model_from_config(home: &Path) -> String {
    if let Some(content) = read_file(home, ".copilot/config.json") {
        if let Some(model) = parse_json_string_value(&content, "preferred_model") {
            if !model.trim().is_empty() {
                return model;
            }
        }
    }
    DEFAULT_MODEL.to_string()
}

fn detect_plan_type(home: &Path) -> String {
    if let Some(content) = read_file(home, ".copilot/config.json") {
        if let Some(plan) = parse_json_string_value(&content, "plan_type") {
            if !plan.trim().is_empty() {
                return plan;
            }
        }
        if let Some(sub) = parse_json_string_value(&content, "subscription_type") {
            if !sub.trim().is_empty() {
                return sub;
            }
        }
    }
    "unknown".to_string()
}

fn file_modified_at(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn update_latest_time(latest: &mut Option<SystemTime>, candidate: Option<SystemTime>) {
    let Some(candidate) = candidate else { return };
    match latest {
        Some(existing) if candidate > *existing => *existing = candidate,
        None => *latest = Some(candidate),
        _ => {}
    }
}

fn format_optional_timestamp(time: Option<SystemTime>) -> String {
    let Some(time) = time else {
        return "-".to_string();
    };
    let Ok(duration) = time.duration_since(UNIX_EPOCH) else {
        return "-".to_string();
    };
    let secs = duration.as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let days = secs / 86400;
    let year = 1970 + days / 365;
    let rem = days % 365;
    let month = rem / 30 + 1;
    let day = rem % 30 + 1;
    format!("{year}-{month:02}-{day:02} {h:02}:{m:02} UTC")
}

fn read_file(home: &Path, relative_path: &str) -> Option<String> {
    fs::read_to_string(home.join(relative_path)).ok()
}

fn unavailable_summary() -> CopilotUsageSummary {
    CopilotUsageSummary {
        availability: UsageAvailability::Unavailable,
        model: DEFAULT_MODEL.to_string(),
        plan_type: "unknown".to_string(),
        last_active_at: "-".to_string(),
        total_requests: None,
        last_24h_requests: None,
        total_sessions: None,
        remaining_percent: None,
        hint: HINT.to_string(),
        quota: None,
    }
}

fn default_runner(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
}

fn copilot_api_url() -> String {
    let port = env::var("PUP_COPILOT_API_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_API_PORT);
    format!("http://localhost:{port}/usage")
}

fn fetch_copilot_api_quota(
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
) -> Option<CopilotQuotaInfo> {
    let url = copilot_api_url();
    fetch_copilot_api_quota_from_url(runner, &url)
}

fn fetch_copilot_api_quota_from_url(
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
    url: &str,
) -> Option<CopilotQuotaInfo> {
    let body = runner(
        "curl",
        &[
            "-fsSL",
            "--connect-timeout",
            API_CONNECT_TIMEOUT_SECS,
            "--max-time",
            API_TIMEOUT_SECS,
            url,
        ],
    )?;

    parse_copilot_api_response(&body)
}

fn parse_copilot_api_response(body: &str) -> Option<CopilotQuotaInfo> {
    let root: Value = serde_json::from_str(body).ok()?;

    let login = root.get("login")?.as_str()?.to_string();
    let plan = root
        .get("copilot_plan")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let reset_date = root
        .get("quota_reset_date")
        .and_then(|v| v.as_str())
        .unwrap_or("-")
        .to_string();

    let snapshots = root.get("quota_snapshots")?;

    let premium = parse_quota_entry(snapshots.get("premium_interactions")?)?;
    let chat = parse_quota_entry(snapshots.get("chat")?)?;
    let completions = parse_quota_entry(snapshots.get("completions")?)?;

    Some(CopilotQuotaInfo {
        login,
        plan,
        reset_date,
        premium,
        chat,
        completions,
    })
}

fn parse_quota_entry(value: &Value) -> Option<CopilotQuotaEntry> {
    let entitlement = value.get("entitlement")?.as_u64().unwrap_or(0);
    let remaining = value.get("remaining")?.as_u64().unwrap_or(0);
    let percent_f = value
        .get("percent_remaining")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let unlimited = value
        .get("unlimited")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Some(CopilotQuotaEntry {
        entitlement,
        remaining,
        percent_remaining_x10: (percent_f * 10.0) as u64,
        unlimited,
    })
}

fn parse_json_string_value(content: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{key}\"");
    let key_start = content.find(&key_pattern)?;
    let rest = &content[key_start + key_pattern.len()..];
    let colon_index = rest.find(':')?;
    parse_quoted_string(rest[colon_index + 1..].trim_start())
}

fn parse_quoted_string(input: &str) -> Option<String> {
    let mut chars = input.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut output = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(output),
            '\\' => {
                let escaped = chars.next()?;
                match escaped {
                    '"' | '\\' | '/' => output.push(escaped),
                    'b' => output.push('\u{0008}'),
                    'f' => output.push('\u{000C}'),
                    'n' => output.push('\n'),
                    'r' => output.push('\r'),
                    't' => output.push('\t'),
                    other => output.push(other),
                }
            }
            other => output.push(other),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{collect_copilot_usage_summary_with_home, parse_copilot_api_response};
    use crate::model::UsageAvailability;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "pup-cli-copilot-{prefix}-{}-{ts}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn write_file(&self, rel: &str, content: &str) {
            let path = self.path.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn fixed_now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    fn recent_event(event_type: &str) -> String {
        format!(r#"{{"type":"{event_type}","data":{{"model":"claude-sonnet-4.6"}},"timestamp":"2027-01-14T02:13:20.000Z"}}"#)
    }

    fn sample_api_response() -> &'static str {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/copilot-api-usage.json"
        ))
    }

    #[test]
    fn falls_back_to_defaults_when_home_is_missing() {
        let summary = collect_copilot_usage_summary_with_home(None, fixed_now());
        assert_eq!(summary.availability, UsageAvailability::Unavailable);
        assert_eq!(summary.model, "claude-sonnet-4-6");
        assert!(summary.quota.is_none());
    }

    #[test]
    fn falls_back_to_defaults_when_no_sessions_dir() {
        let home = TestDir::new("copilot-empty");
        let summary = collect_copilot_usage_summary_with_home(Some(&home.path), fixed_now());
        assert_eq!(summary.availability, UsageAvailability::Unavailable);
        assert!(summary.total_requests.is_none());
    }

    #[test]
    fn counts_requests_from_turn_start_events() {
        let home = TestDir::new("copilot-turns");
        let events = format!(
            "{}\n{}\n{}\n",
            recent_event("assistant.turn_start"),
            recent_event("assistant.message"),
            recent_event("assistant.turn_start"),
        );
        home.write_file(".copilot/session-state/sess-a/events.jsonl", &events);

        let summary = collect_copilot_usage_summary_with_home(Some(&home.path), fixed_now());
        assert_eq!(summary.availability, UsageAvailability::Live);
        assert_eq!(summary.total_requests, Some(2));
        assert_eq!(summary.total_sessions, Some(1));
    }

    #[test]
    fn extracts_model_from_message_events() {
        let home = TestDir::new("copilot-model");
        home.write_file(
            ".copilot/session-state/sess-a/events.jsonl",
            &format!("{}\n", recent_event("assistant.message")),
        );
        let summary = collect_copilot_usage_summary_with_home(Some(&home.path), fixed_now());
        assert_eq!(summary.model, "claude-sonnet-4.6");
    }

    #[test]
    fn reads_plan_type_from_config() {
        let home = TestDir::new("copilot-plan");
        home.write_file(
            ".copilot/config.json",
            r#"{"plan_type": "Individual"}"#,
        );
        let summary = collect_copilot_usage_summary_with_home(Some(&home.path), fixed_now());
        assert_eq!(summary.plan_type, "Individual");
    }

    #[test]
    fn parses_api_response_quota_info() {
        let info = parse_copilot_api_response(sample_api_response()).unwrap();
        assert_eq!(info.login, "pengxu-liu_nioer");
        assert_eq!(info.plan, "business");
        assert_eq!(info.reset_date, "2026-05-01");

        assert!(info.chat.unlimited);
        assert!(info.completions.unlimited);
        assert!(!info.premium.unlimited);

        assert_eq!(info.premium.entitlement, 300);
        assert_eq!(info.premium.remaining, 287);
        assert_eq!(info.premium.percent_remaining_x10, 956); // 95.6 * 10
    }

    #[test]
    fn parses_api_response_returns_none_for_invalid_json() {
        assert!(parse_copilot_api_response("not json").is_none());
        assert!(parse_copilot_api_response("{}").is_none());
    }

    #[test]
    fn fetch_returns_none_when_runner_fails() {
        let mut runner = |_program: &str, _args: &[&str]| -> Option<String> { None };
        let result = super::fetch_copilot_api_quota_from_url(
            &mut runner,
            "http://localhost:9999/usage",
        );
        assert!(result.is_none());
    }

    #[test]
    fn fetch_parses_runner_output() {
        let body = sample_api_response().to_string();
        let mut runner = move |_program: &str, _args: &[&str]| -> Option<String> {
            Some(body.clone())
        };
        let result = super::fetch_copilot_api_quota_from_url(
            &mut runner,
            "http://localhost:1414/usage",
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().plan, "business");
    }
}
