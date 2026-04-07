use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::model::{CopilotQuotaEntry, CopilotQuotaInfo, CopilotUsageSummary, UsageAvailability};

const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
const HINT: &str = "Set PUP_GITHUB_TOKEN or run `pupkit auth` to fetch Copilot quota details";
const DAY_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const EVENT_TURN_START: &str = "assistant.turn_start";
const EVENT_MESSAGE: &str = "assistant.message";
const GITHUB_BASE_URL: &str = "https://github.com";
const GITHUB_API_BASE_URL: &str = "https://api.github.com";
// OAuth client IDs are public app identifiers, not client secrets.
const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const GITHUB_APP_SCOPES: &str = "read:user";
const EDITOR_PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
const API_VERSION: &str = "2025-04-01";
const VSCODE_VERSION: &str = "1.104.3";
const CURL_CONNECT_TIMEOUT_SECS: &str = "2";
const CURL_MAX_TIME_SECS: &str = "10";
const DEVICE_AUTH_ENV: &str = "PUP_COPILOT_DEVICE_AUTH";
const PUP_GITHUB_TOKEN_ENV: &str = "PUP_GITHUB_TOKEN";
const GITHUB_TOKEN_ENV: &str = "GITHUB_TOKEN";
const GH_TOKEN_ENV: &str = "GH_TOKEN";
const PUPKIT_GITHUB_TOKEN_PATH: &str = ".local/share/pupkit/github_token";
const PROXY_ENV_KEYS: [&str; 6] = [
    "http_proxy",
    "HTTP_PROXY",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
];

pub fn collect_copilot_usage_summary() -> CopilotUsageSummary {
    finish_copilot_usage_summary(collect_copilot_usage_summary_fast())
}

pub fn collect_copilot_usage_summary_fast() -> CopilotUsageSummary {
    let home = env::var_os("HOME").map(PathBuf::from);
    collect_copilot_usage_summary_fast_with_home(home.as_deref(), SystemTime::now())
}

pub fn finish_copilot_usage_summary(summary: CopilotUsageSummary) -> CopilotUsageSummary {
    let home = env::var_os("HOME").map(PathBuf::from);
    hydrate_copilot_usage_summary(summary, home.as_deref())
}

pub fn run_github_auth_flow() -> Result<PathBuf, String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set; cannot determine token cache path".to_string())?;

    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err("auth command requires an interactive terminal".to_string());
    }

    let device_code =
        request_device_code().ok_or_else(|| "failed to request GitHub device code".to_string())?;
    eprintln!(
        "Copilot auth required. Open {} and enter code {}.",
        device_code.verification_uri, device_code.user_code
    );
    let _ = io::stderr().flush();

    let token = poll_access_token(&device_code)
        .ok_or_else(|| "failed to complete GitHub device authorization".to_string())?;
    let token_path = primary_github_token_path(Some(home.as_path()))
        .ok_or_else(|| "failed to determine pupkit token cache path".to_string())?;
    write_github_token_cache(Some(home.as_path()), &token)
        .ok_or_else(|| "failed to write GitHub token cache".to_string())?;

    Ok(token_path)
}

fn collect_copilot_usage_summary_fast_with_home(
    home: Option<&Path>,
    now: SystemTime,
) -> CopilotUsageSummary {
    let mut summary = collect_copilot_usage_summary_with_home(home, now);
    summary.is_loading = true;
    summary
}

fn hydrate_copilot_usage_summary(
    mut summary: CopilotUsageSummary,
    home: Option<&Path>,
) -> CopilotUsageSummary {
    if let Some(quota) = fetch_copilot_quota(home) {
        summary.plan_type = quota.plan.clone();
        summary.availability = match summary.availability {
            UsageAvailability::Unavailable => UsageAvailability::Partial,
            other => other,
        };
        summary.quota = Some(quota);
    }

    summary.is_loading = false;
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
        is_loading: false,
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PollAccessTokenStatus {
    Authorized(String),
    Pending,
    SlowDown,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CurlCommandSpec {
    args: Vec<String>,
    stdin_payload: String,
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
            EVENT_TURN_START => {
                aggregate.total_requests += 1;
                if in_last_24h {
                    aggregate.last_24h_requests += 1;
                }
            }
            EVENT_MESSAGE => {
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
        let is_newer = match (&aggregate.latest_activity_at, file_mtime) {
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
        is_loading: false,
        last_active_at: "-".to_string(),
        total_requests: None,
        last_24h_requests: None,
        total_sessions: None,
        remaining_percent: None,
        hint: HINT.to_string(),
        quota: None,
    }
}

fn fetch_copilot_quota(home: Option<&Path>) -> Option<CopilotQuotaInfo> {
    let token = ensure_github_token(home)?;
    let body = fetch_copilot_usage_body(&token)?;
    parse_copilot_api_response(&body)
}

fn ensure_github_token(home: Option<&Path>) -> Option<String> {
    read_github_token_from_sources(home).or_else(|| authenticate_github_token(home))
}

fn read_github_token_from_sources(home: Option<&Path>) -> Option<String> {
    read_token_env(PUP_GITHUB_TOKEN_ENV)
        .or_else(|| read_token_env(GITHUB_TOKEN_ENV))
        .or_else(|| read_token_env(GH_TOKEN_ENV))
        .or_else(|| read_github_token_file(primary_github_token_path(home).as_deref()))
}

fn read_token_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_github_token_file(path: Option<&Path>) -> Option<String> {
    let path = path?;
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn authenticate_github_token(home: Option<&Path>) -> Option<String> {
    if !should_attempt_device_auth() {
        return None;
    }

    let device_code = request_device_code()?;
    eprintln!(
        "Copilot auth required. Open {} and enter code {}.",
        device_code.verification_uri, device_code.user_code
    );
    let _ = io::stderr().flush();

    let token = poll_access_token(&device_code)?;
    let _ = write_github_token_cache(home, &token);
    Some(token)
}

fn should_attempt_device_auth() -> bool {
    env_flag_is_truthy(DEVICE_AUTH_ENV) && io::stdin().is_terminal() && io::stderr().is_terminal()
}

fn env_flag_is_truthy(key: &str) -> bool {
    env::var(key)
        .ok()
        .is_some_and(|value| is_truthy_value(&value))
}

fn is_truthy_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn primary_github_token_path(home: Option<&Path>) -> Option<PathBuf> {
    Some(home?.join(PUPKIT_GITHUB_TOKEN_PATH))
}

fn write_github_token_cache(home: Option<&Path>, token: &str) -> Option<()> {
    let path = primary_github_token_path(home)?;
    let parent = path.parent()?;
    ensure_private_dir(parent).ok()?;
    write_secret_file(&path, token).ok()?;
    Some(())
}

fn request_device_code() -> Option<DeviceCodeResponse> {
    let body = run_curl_json_request(
        "POST",
        &format!("{GITHUB_BASE_URL}/login/device/code"),
        &standard_headers(),
        Some(json!({
            "client_id": GITHUB_CLIENT_ID,
            "scope": GITHUB_APP_SCOPES,
        })),
    )?;

    parse_device_code_response(&body)
}

fn poll_access_token(device_code: &DeviceCodeResponse) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(device_code.expires_in);
    let mut sleep_secs = device_code.interval.saturating_add(1);

    while Instant::now() < deadline {
        let body = run_curl_json_request(
            "POST",
            &format!("{GITHUB_BASE_URL}/login/oauth/access_token"),
            &standard_headers(),
            Some(json!({
                "client_id": GITHUB_CLIENT_ID,
                "device_code": device_code.device_code,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            })),
        )?;

        match parse_access_token_poll_response(&body) {
            PollAccessTokenStatus::Authorized(token) => return Some(token),
            PollAccessTokenStatus::Pending => {}
            PollAccessTokenStatus::SlowDown => {
                sleep_secs = sleep_secs.saturating_add(5);
            }
            PollAccessTokenStatus::Failed => return None,
        }

        thread::sleep(Duration::from_secs(sleep_secs));
    }

    None
}

fn fetch_copilot_usage_body(token: &str) -> Option<String> {
    let mut headers = github_headers(token);
    headers.push(format!("editor-version: vscode/{VSCODE_VERSION}"));
    run_curl_json_request(
        "GET",
        &format!("{GITHUB_API_BASE_URL}/copilot_internal/user"),
        &headers,
        None,
    )
}

fn standard_headers() -> Vec<String> {
    vec![
        "accept: application/json".to_string(),
        "content-type: application/json".to_string(),
    ]
}

fn github_headers(token: &str) -> Vec<String> {
    let mut headers = standard_headers();
    headers.push(format!("authorization: token {token}"));
    headers.push(format!("editor-plugin-version: {EDITOR_PLUGIN_VERSION}"));
    headers.push(format!("user-agent: {USER_AGENT}"));
    headers.push(format!("x-github-api-version: {API_VERSION}"));
    headers.push("x-vscode-user-agent-library-version: electron-fetch".to_string());
    headers
}

fn run_curl_json_request(
    method: &str,
    url: &str,
    headers: &[String],
    body: Option<Value>,
) -> Option<String> {
    let spec = build_curl_command_spec(method, url, headers, body.as_ref());

    run_curl_command(&spec, false).or_else(|| {
        if has_proxy_env() {
            run_curl_command(&spec, true)
        } else {
            None
        }
    })
}

fn build_curl_command_spec(
    method: &str,
    url: &str,
    headers: &[String],
    body: Option<&Value>,
) -> CurlCommandSpec {
    let mut stdin_payload = String::new();
    stdin_payload.push_str("silent\n");
    stdin_payload.push_str("show-error\n");
    stdin_payload.push_str("fail\n");
    stdin_payload.push_str("location\n");
    append_curl_config_line(
        &mut stdin_payload,
        "connect-timeout",
        CURL_CONNECT_TIMEOUT_SECS,
    );
    append_curl_config_line(&mut stdin_payload, "max-time", CURL_MAX_TIME_SECS);
    append_curl_config_line(&mut stdin_payload, "request", method);

    for header in headers {
        append_curl_config_line(&mut stdin_payload, "header", header);
    }

    if let Some(body) = body {
        append_curl_config_line(&mut stdin_payload, "data", &body.to_string());
    }

    append_curl_config_line(&mut stdin_payload, "url", url);

    CurlCommandSpec {
        args: vec!["--config".to_string(), "-".to_string()],
        stdin_payload,
    }
}

fn append_curl_config_line(buffer: &mut String, key: &str, value: &str) {
    buffer.push_str(key);
    buffer.push_str(" = \"");
    buffer.push_str(&escape_curl_config_string(value));
    buffer.push_str("\"\n");
}

fn escape_curl_config_string(value: &str) -> String {
    let mut escaped = String::new();

    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }

    escaped
}

fn has_proxy_env() -> bool {
    PROXY_ENV_KEYS.iter().any(|key| {
        env::var(key)
            .ok()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

fn run_curl_command(spec: &CurlCommandSpec, clear_proxy_env: bool) -> Option<String> {
    let mut command = Command::new("curl");
    command
        .args(spec.args.iter().map(String::as_str))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if clear_proxy_env {
        for key in PROXY_ENV_KEYS {
            command.env_remove(key);
        }
    }

    let mut child = command.spawn().ok()?;
    {
        let mut stdin = child.stdin.take()?;
        stdin.write_all(spec.stdin_payload.as_bytes()).ok()?;
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_secret_file(path: &Path, contents: &str) -> io::Result<()> {
    #[cfg(unix)]
    if path.exists() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    Ok(())
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

    Some(CopilotQuotaInfo {
        login,
        plan,
        reset_date,
        premium: parse_quota_entry(snapshots.get("premium_interactions")?)?,
        chat: parse_quota_entry(snapshots.get("chat")?)?,
        completions: parse_quota_entry(snapshots.get("completions")?)?,
    })
}

fn parse_quota_entry(value: &Value) -> Option<CopilotQuotaEntry> {
    Some(CopilotQuotaEntry {
        entitlement: value.get("entitlement").and_then(json_u64).unwrap_or(0),
        remaining: value.get("remaining").and_then(json_u64).unwrap_or(0),
        percent_remaining_x10: value
            .get("percent_remaining")
            .and_then(|v| v.as_f64())
            .map(|percent| (percent * 10.0) as u64)
            .unwrap_or(0),
        unlimited: value
            .get("unlimited")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|number| number as u64))
}

fn parse_device_code_response(body: &str) -> Option<DeviceCodeResponse> {
    let root: Value = serde_json::from_str(body).ok()?;

    Some(DeviceCodeResponse {
        device_code: root.get("device_code")?.as_str()?.to_string(),
        user_code: root.get("user_code")?.as_str()?.to_string(),
        verification_uri: root.get("verification_uri")?.as_str()?.to_string(),
        expires_in: root.get("expires_in")?.as_u64()?,
        interval: root.get("interval")?.as_u64()?,
    })
}

fn parse_access_token_poll_response(body: &str) -> PollAccessTokenStatus {
    let Ok(root): Result<Value, _> = serde_json::from_str(body) else {
        return PollAccessTokenStatus::Failed;
    };

    if let Some(access_token) = root.get("access_token").and_then(|v| v.as_str()) {
        return PollAccessTokenStatus::Authorized(access_token.to_string());
    }

    match root.get("error").and_then(|v| v.as_str()) {
        Some("authorization_pending") => PollAccessTokenStatus::Pending,
        Some("slow_down") => PollAccessTokenStatus::SlowDown,
        _ => PollAccessTokenStatus::Failed,
    }
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
    use super::{
        PollAccessTokenStatus, build_curl_command_spec,
        collect_copilot_usage_summary_fast_with_home, collect_copilot_usage_summary_with_home,
        is_truthy_value, parse_access_token_poll_response, parse_copilot_api_response,
        parse_device_code_response, primary_github_token_path, read_github_token_file,
        write_github_token_cache,
    };
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
        format!(
            r#"{{"type":"{event_type}","data":{{"model":"claude-sonnet-4.6"}},"timestamp":"2027-01-14T02:13:20.000Z"}}"#
        )
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
        assert!(!summary.is_loading);
    }

    #[test]
    fn falls_back_to_defaults_when_no_sessions_dir() {
        let home = TestDir::new("copilot-empty");
        let summary = collect_copilot_usage_summary_with_home(Some(&home.path), fixed_now());
        assert_eq!(summary.availability, UsageAvailability::Unavailable);
        assert!(summary.total_requests.is_none());
        assert!(!summary.is_loading);
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
        assert!(!summary.is_loading);
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
        home.write_file(".copilot/config.json", r#"{"plan_type": "Individual"}"#);
        let summary = collect_copilot_usage_summary_with_home(Some(&home.path), fixed_now());
        assert_eq!(summary.plan_type, "Individual");
        assert!(!summary.is_loading);
    }

    #[test]
    fn fast_summary_marks_quota_as_loading() {
        let home = TestDir::new("copilot-fast");
        let summary = collect_copilot_usage_summary_fast_with_home(Some(&home.path), fixed_now());

        assert!(summary.is_loading);
        assert!(summary.quota.is_none());
    }

    #[test]
    fn reads_primary_github_token_cache() {
        let home = TestDir::new("github-token-primary");
        let path = primary_github_token_path(Some(home.path.as_path())).unwrap();
        home.write_file(".local/share/pupkit/github_token", "ghu_primary\n");

        assert_eq!(read_github_token_file(Some(&path)).unwrap(), "ghu_primary");
    }

    #[test]
    fn curl_command_spec_keeps_token_out_of_args() {
        let spec = build_curl_command_spec(
            "GET",
            "https://api.github.com/copilot_internal/user",
            &["authorization: token ghu_secret".to_string()],
            None,
        );

        assert_eq!(spec.args, vec!["--config".to_string(), "-".to_string()]);
        assert!(!spec.args.iter().any(|arg| arg.contains("ghu_secret")));
        assert!(
            spec.stdin_payload
                .contains("authorization: token ghu_secret")
        );
        assert!(
            spec.stdin_payload
                .contains("url = \"https://api.github.com/copilot_internal/user\"")
        );
    }

    #[cfg(unix)]
    #[test]
    fn github_token_cache_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let home = TestDir::new("github-token-perms");
        let token_path = primary_github_token_path(Some(home.path.as_path())).unwrap();

        write_github_token_cache(Some(home.path.as_path()), "ghu_secret").unwrap();

        let dir_mode = std::fs::metadata(token_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = std::fs::metadata(&token_path).unwrap().permissions().mode() & 0o777;

        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        assert_eq!(std::fs::read_to_string(token_path).unwrap(), "ghu_secret");
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
        assert_eq!(info.premium.percent_remaining_x10, 956);
    }

    #[test]
    fn parses_api_response_returns_none_for_invalid_json() {
        assert!(parse_copilot_api_response("not json").is_none());
        assert!(parse_copilot_api_response("{}").is_none());
    }

    #[test]
    fn parses_device_code_response_shape() {
        let response = parse_device_code_response(
            r#"{
                "device_code": "dev-code",
                "user_code": "USER-CODE",
                "verification_uri": "https://github.com/login/device",
                "expires_in": 900,
                "interval": 5
            }"#,
        )
        .unwrap();

        assert_eq!(response.device_code, "dev-code");
        assert_eq!(response.user_code, "USER-CODE");
        assert_eq!(response.verification_uri, "https://github.com/login/device");
        assert_eq!(response.expires_in, 900);
        assert_eq!(response.interval, 5);
    }

    #[test]
    fn parses_access_token_poll_states() {
        assert_eq!(
            parse_access_token_poll_response(r#"{"access_token":"ghu_token"}"#),
            PollAccessTokenStatus::Authorized("ghu_token".to_string())
        );
        assert_eq!(
            parse_access_token_poll_response(r#"{"error":"authorization_pending"}"#),
            PollAccessTokenStatus::Pending
        );
        assert_eq!(
            parse_access_token_poll_response(r#"{"error":"slow_down"}"#),
            PollAccessTokenStatus::SlowDown
        );
        assert_eq!(
            parse_access_token_poll_response(r#"{"error":"expired_token"}"#),
            PollAccessTokenStatus::Failed
        );
    }

    #[test]
    fn truthy_value_parser_matches_expected_values() {
        assert!(is_truthy_value("true"));
        assert!(is_truthy_value("YES"));
        assert!(is_truthy_value(" 1 "));
        assert!(!is_truthy_value("0"));
        assert!(!is_truthy_value("false"));
    }
}
