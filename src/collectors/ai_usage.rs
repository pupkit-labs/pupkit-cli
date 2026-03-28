use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::model::{
    AiUsageSummary, ClaudeUsageSummary, CodexUsageSummary, RateLimitWindow, TokenBreakdown,
    UsageAvailability,
};

const CLAUDE_SOURCE_LABEL: &str = "local jsonl aggregate";
const UNAVAILABLE_LABEL: &str = "unavailable";
const UNKNOWN_PLAN_LABEL: &str = "unknown";
const PLACEHOLDER_LABEL: &str = "-";
const PRIMARY_WINDOW_LABEL: &str = "Primary";
const SECONDARY_WINDOW_LABEL: &str = "Secondary";
const CLAUDE_HINT: &str = "Run /usage or /stats in Claude for plan limits";
const CODEX_HINT: &str = "Run /status in Codex for current usage";
const DAY_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const WEEK_WINDOW: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const AUTH_PLAN_PATHS: &[&[&str]] = &[
    &["plan_type"],
    &["chatgpt_plan_type"],
    &["claims", "plan_type"],
    &["claims", "chatgpt_plan_type"],
    &["id_token", "claims", "plan_type"],
    &["id_token", "claims", "chatgpt_plan_type"],
    &["tokens", "id_token", "claims", "plan_type"],
    &["tokens", "id_token", "claims", "chatgpt_plan_type"],
    &["https://api.openai.com/auth", "plan_type"],
    &["https://api.openai.com/auth", "chatgpt_plan_type"],
    &["https://api.openai.com/auth", "claims", "plan_type"],
    &["https://api.openai.com/auth", "claims", "chatgpt_plan_type"],
];
const AUTH_JWT_TOKEN_PATHS: &[&[&str]] = &[&["tokens", "id_token"], &["id_token"]];

pub fn collect_ai_usage_summary() -> AiUsageSummary {
    let home = env::var_os("HOME").map(PathBuf::from);
    collect_ai_usage_summary_with_home(home.as_deref(), SystemTime::now())
}

fn collect_ai_usage_summary_with_home(home: Option<&Path>, now: SystemTime) -> AiUsageSummary {
    let mut warnings = Vec::new();
    let claude = collect_claude_usage_summary(home, now, &mut warnings);
    let codex = collect_codex_usage_summary(home, now, &mut warnings);

    AiUsageSummary {
        claude,
        codex,
        warnings,
    }
}

fn collect_claude_usage_summary(
    home: Option<&Path>,
    now: SystemTime,
    warnings: &mut Vec<String>,
) -> ClaudeUsageSummary {
    let Some(home) = home else {
        return unavailable_claude_summary();
    };

    let projects_dir = home.join(".claude/projects");
    if !projects_dir.is_dir() {
        return unavailable_claude_summary();
    }

    let mut aggregate = ClaudeAggregate::default();

    let Ok(entries) = fs::read_dir(&projects_dir) else {
        return unavailable_claude_summary();
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_jsonl_path(&path) {
            continue;
        }

        aggregate.has_jsonl_files = true;
        let file_mtime = file_modified_at(&path);
        update_latest_time(&mut aggregate.latest_activity_at, file_mtime);

        if let Err(error) = process_claude_jsonl_file(&path, file_mtime, now, &mut aggregate) {
            if error.count_as_unreadable_file {
                aggregate.unreadable_files += 1;
            }
        }
    }

    append_line_warning(
        warnings,
        "Claude",
        aggregate.malformed_lines,
        "malformed lines",
    );
    append_line_warning(
        warnings,
        "Claude",
        aggregate.unreadable_files,
        "unreadable files",
    );

    if aggregate.usage_records > 0 {
        finalize_breakdown(&mut aggregate.last_24h);
        finalize_breakdown(&mut aggregate.last_7d);
        finalize_breakdown(&mut aggregate.lifetime);
    }

    ClaudeUsageSummary {
        availability: if aggregate.usage_records > 0 {
            UsageAvailability::Live
        } else {
            UsageAvailability::Partial
        },
        source_label: CLAUDE_SOURCE_LABEL.to_string(),
        last_active_at: format_optional_timestamp(aggregate.latest_activity_at),
        last_24h: aggregate.last_24h,
        last_7d: aggregate.last_7d,
        lifetime: aggregate.lifetime,
        hint: CLAUDE_HINT.to_string(),
    }
}

fn collect_codex_usage_summary(
    home: Option<&Path>,
    now: SystemTime,
    warnings: &mut Vec<String>,
) -> CodexUsageSummary {
    let Some(home) = home else {
        return unavailable_codex_summary();
    };

    let sessions_dir = home.join(".codex/sessions");
    let auth_path = home.join(".codex/auth.json");
    let mut aggregate = CodexAggregate::default();

    if sessions_dir.is_dir() {
        aggregate.has_sessions_dir = true;
        scan_codex_sessions_dir(&sessions_dir, now, &mut aggregate);
    }

    append_line_warning(
        warnings,
        "Codex",
        aggregate.malformed_lines,
        "malformed lines",
    );
    append_line_warning(
        warnings,
        "Codex",
        aggregate.unreadable_files,
        "unreadable files",
    );

    let mut auth_warning_needed = false;
    let plan_type = if let Some(plan_type) = aggregate.plan_from_session.clone() {
        plan_type
    } else {
        match read_codex_auth_plan_type(&auth_path) {
            AuthPlanRead::Found(plan_type) => plan_type,
            AuthPlanRead::Unavailable => {
                auth_warning_needed = true;
                UNKNOWN_PLAN_LABEL.to_string()
            }
            AuthPlanRead::Missing => UNKNOWN_PLAN_LABEL.to_string(),
        }
    };

    if auth_warning_needed {
        warnings.push("Codex auth plan type unavailable".to_string());
    }

    CodexUsageSummary {
        availability: if aggregate.token_records > 0 {
            UsageAvailability::Live
        } else if aggregate.has_sessions_dir || plan_type != UNKNOWN_PLAN_LABEL {
            UsageAvailability::Partial
        } else {
            UsageAvailability::Unavailable
        },
        plan_type,
        last_active_at: format_optional_timestamp(aggregate.latest_activity_at),
        last_session_total_tokens: aggregate.last_session_total_tokens,
        last_24h_total_tokens: if aggregate.token_records > 0 {
            Some(aggregate.last_24h_total_tokens)
        } else {
            None
        },
        last_7d_total_tokens: if aggregate.token_records > 0 {
            Some(aggregate.last_7d_total_tokens)
        } else {
            None
        },
        primary_rate_limit: aggregate
            .primary_rate_limit
            .unwrap_or_else(default_primary_rate_limit),
        secondary_rate_limit: aggregate
            .secondary_rate_limit
            .unwrap_or_else(default_secondary_rate_limit),
        hint: CODEX_HINT.to_string(),
    }
}

fn process_claude_jsonl_file(
    path: &Path,
    file_mtime: Option<SystemTime>,
    now: SystemTime,
    aggregate: &mut ClaudeAggregate,
) -> Result<(), FileProcessError> {
    let file = File::open(path).map_err(|_| FileProcessError::unreadable())?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => return Err(FileProcessError::unreadable()),
        };

        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                aggregate.malformed_lines += 1;
                continue;
            }
        };

        let timestamp = extract_record_timestamp(&value).or(file_mtime);
        update_latest_time(&mut aggregate.latest_activity_at, timestamp);

        let Some(tokens) = extract_claude_tokens(&value) else {
            continue;
        };

        aggregate.usage_records += 1;
        add_token_breakdown(&mut aggregate.lifetime, &tokens);

        if let Some(timestamp) = timestamp {
            if is_within_window(timestamp, now, DAY_WINDOW) {
                add_token_breakdown(&mut aggregate.last_24h, &tokens);
            }
            if is_within_window(timestamp, now, WEEK_WINDOW) {
                add_token_breakdown(&mut aggregate.last_7d, &tokens);
            }
        }
    }

    Ok(())
}

fn scan_codex_sessions_dir(root: &Path, now: SystemTime, aggregate: &mut CodexAggregate) {
    let mut pending = vec![root.to_path_buf()];

    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            aggregate.unreadable_files += 1;
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                aggregate.unreadable_files += 1;
                continue;
            };

            if file_type.is_dir() {
                pending.push(path);
                continue;
            }

            if !file_type.is_file() || !is_jsonl_path(&path) {
                continue;
            }

            aggregate.has_session_files = true;
            let file_mtime = file_modified_at(&path);
            update_latest_time(&mut aggregate.latest_activity_at, file_mtime);

            if let Err(error) = process_codex_jsonl_file(&path, file_mtime, now, aggregate) {
                if error.count_as_unreadable_file {
                    aggregate.unreadable_files += 1;
                }
            }
        }
    }
}

fn process_codex_jsonl_file(
    path: &Path,
    file_mtime: Option<SystemTime>,
    now: SystemTime,
    aggregate: &mut CodexAggregate,
) -> Result<(), FileProcessError> {
    let file = File::open(path).map_err(|_| FileProcessError::unreadable())?;
    let reader = BufReader::new(file);
    let mut previous_total = 0_u64;
    let mut seen_total = false;
    let mut session_latest_total = None;
    let mut session_latest_total_at = file_mtime;

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => return Err(FileProcessError::unreadable()),
        };

        if line.trim().is_empty() {
            continue;
        }

        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                aggregate.malformed_lines += 1;
                continue;
            }
        };

        let timestamp = extract_record_timestamp(&value).or(file_mtime);
        update_latest_time(&mut aggregate.latest_activity_at, timestamp);

        let Some(event) = codex_token_event(&value) else {
            continue;
        };

        aggregate.token_records += 1;
        let current_total = extract_codex_usage_total(event, "total_token_usage")
            .or_else(|| extract_codex_usage_total(&value, "total_token_usage"));
        let last_token_usage = extract_codex_usage_total(event, "last_token_usage")
            .or_else(|| extract_codex_usage_total(&value, "last_token_usage"));

        let delta = if let Some(last_token_usage) = last_token_usage {
            last_token_usage
        } else if let Some(current_total) = current_total {
            if seen_total {
                current_total.saturating_sub(previous_total)
            } else {
                current_total
            }
        } else {
            0
        };

        if let Some(timestamp) = timestamp {
            if is_within_window(timestamp, now, DAY_WINDOW) {
                aggregate.last_24h_total_tokens += delta;
            }
            if is_within_window(timestamp, now, WEEK_WINDOW) {
                aggregate.last_7d_total_tokens += delta;
            }
        }

        if let Some(current_total) = current_total {
            previous_total = current_total;
            seen_total = true;
            session_latest_total = Some(current_total);
            session_latest_total_at = timestamp.or(file_mtime);
        }

        if let Some(snapshot) = extract_rate_limit_snapshot(event, &value) {
            update_optional_snapshot(
                &mut aggregate.primary_rate_limit,
                &mut aggregate.primary_rate_limit_at,
                timestamp,
                snapshot.primary_rate_limit,
            );
            update_optional_snapshot(
                &mut aggregate.secondary_rate_limit,
                &mut aggregate.secondary_rate_limit_at,
                timestamp,
                snapshot.secondary_rate_limit,
            );

            if let Some(plan_type) = snapshot.plan_type {
                update_optional_string(
                    &mut aggregate.plan_from_session,
                    &mut aggregate.plan_from_session_at,
                    timestamp,
                    plan_type,
                );
            }
        }
    }

    if let Some(session_latest_total) = session_latest_total {
        update_optional_u64(
            &mut aggregate.last_session_total_tokens,
            &mut aggregate.last_session_total_at,
            session_latest_total_at,
            session_latest_total,
        );
    }

    Ok(())
}

fn read_codex_auth_plan_type(path: &Path) -> AuthPlanRead {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return AuthPlanRead::Missing,
        Err(_) => return AuthPlanRead::Unavailable,
    };

    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return AuthPlanRead::Unavailable;
    };

    if let Some(plan_type) = find_plan_type_in_value(&value) {
        return AuthPlanRead::Found(plan_type);
    }

    for path in AUTH_JWT_TOKEN_PATHS {
        if let Some(plan_type) = get_string_at_path(&value, path)
            .and_then(decode_jwt_claims)
            .and_then(|claims| find_plan_type_in_value(&claims))
        {
            return AuthPlanRead::Found(plan_type);
        }
    }

    AuthPlanRead::Unavailable
}

fn extract_claude_tokens(value: &Value) -> Option<TokenBreakdown> {
    let usage = value.get("usage")?;
    let input_tokens = get_u64_field(usage, "input_tokens");
    let output_tokens = get_u64_field(usage, "output_tokens");
    let cache_creation_input_tokens = get_u64_field(usage, "cache_creation_input_tokens");
    let cache_read_input_tokens = get_u64_field(usage, "cache_read_input_tokens");

    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_creation_input_tokens.is_none()
        && cache_read_input_tokens.is_none()
    {
        return None;
    }

    Some(TokenBreakdown {
        total_tokens: input_tokens.unwrap_or(0)
            + output_tokens.unwrap_or(0)
            + cache_creation_input_tokens.unwrap_or(0)
            + cache_read_input_tokens.unwrap_or(0),
        input_tokens,
        output_tokens,
        cache_creation_input_tokens,
        cache_read_input_tokens,
    })
}

fn extract_rate_limit_snapshot(event: &Value, value: &Value) -> Option<RateLimitSnapshot> {
    let rate_limits = extract_rate_limits(event).or_else(|| extract_rate_limits(value))?;

    Some(RateLimitSnapshot {
        plan_type: get_string_field(rate_limits, "plan_type").and_then(normalize_plan_type),
        primary_rate_limit: extract_rate_limit_window(rate_limits, "primary", PRIMARY_WINDOW_LABEL),
        secondary_rate_limit: extract_rate_limit_window(
            rate_limits,
            "secondary",
            SECONDARY_WINDOW_LABEL,
        ),
    })
}

fn extract_rate_limit_window(
    rate_limits: &Value,
    key: &str,
    label: &'static str,
) -> RateLimitWindow {
    let window = rate_limits.get(key);
    RateLimitWindow {
        label,
        used_percent: window.and_then(|value| get_u8_field(value, "used_percent")),
        resets_at: window
            .and_then(|value| value.get("resets_at"))
            .and_then(format_timestamp_value)
            .unwrap_or_else(|| PLACEHOLDER_LABEL.to_string()),
    }
}

fn codex_token_event(value: &Value) -> Option<&Value> {
    if looks_like_codex_token_payload(value) {
        Some(value)
    } else {
        value
            .get("payload")
            .filter(|payload| looks_like_codex_token_payload(payload))
    }
}

fn looks_like_codex_token_payload(value: &Value) -> bool {
    value.get("token_count").is_some_and(Value::is_object)
        || get_string_field(value, "type").is_some_and(|value| value == "token_count")
        || get_string_field(value, "event").is_some_and(|value| value == "token_count")
        || value.get("total_token_usage").is_some()
        || value.get("last_token_usage").is_some()
        || value.get("rate_limits").is_some()
        || value
            .get("info")
            .and_then(|info| info.get("total_token_usage"))
            .is_some()
        || value
            .get("info")
            .and_then(|info| info.get("last_token_usage"))
            .is_some()
}

fn extract_codex_usage_total(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(parse_codex_usage_total)
        .or_else(|| {
            value
                .get("token_count")
                .and_then(|token_count| token_count.get(key))
                .and_then(parse_codex_usage_total)
        })
        .or_else(|| {
            value
                .get("info")
                .and_then(|info| info.get(key))
                .and_then(parse_codex_usage_total)
        })
        .or_else(|| {
            value
                .get("payload")
                .and_then(|payload| extract_codex_usage_total(payload, key))
        })
}

fn parse_codex_usage_total(value: &Value) -> Option<u64> {
    if value.is_object() {
        get_u64_field(value, "total_tokens").or_else(|| get_u64_field(value, "total_token_usage"))
    } else {
        parse_u64_value(value)
    }
}

fn extract_rate_limits(value: &Value) -> Option<&Value> {
    value
        .get("rate_limits")
        .or_else(|| {
            value
                .get("token_count")
                .and_then(|token_count| token_count.get("rate_limits"))
        })
        .or_else(|| value.get("info").and_then(|info| info.get("rate_limits")))
        .or_else(|| value.get("payload").and_then(extract_rate_limits))
}

fn file_modified_at(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

fn is_jsonl_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

fn extract_record_timestamp(value: &Value) -> Option<SystemTime> {
    ["timestamp", "created_at"]
        .iter()
        .find_map(|key| value.get(*key).and_then(parse_json_timestamp))
}

fn parse_json_timestamp(value: &Value) -> Option<SystemTime> {
    match value {
        Value::Number(number) => number.as_i64().and_then(timestamp_from_integer),
        Value::String(string) => parse_timestamp_string(string),
        _ => None,
    }
}

fn parse_timestamp_string(value: &str) -> Option<SystemTime> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(integer) = trimmed.parse::<i64>() {
        return timestamp_from_integer(integer);
    }

    parse_datetime_with_optional_offset(trimmed)
}

fn format_timestamp_value(value: &Value) -> Option<String> {
    parse_json_timestamp(value)
        .map(format_timestamp_label)
        .or_else(|| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

fn timestamp_from_integer(value: i64) -> Option<SystemTime> {
    let (seconds, nanos) = if value.abs() >= 1_000_000_000_000 {
        let seconds = value.div_euclid(1_000);
        let millis = value.rem_euclid(1_000) as u32;
        (seconds, millis * 1_000_000)
    } else {
        (value, 0)
    };

    if seconds >= 0 {
        Some(UNIX_EPOCH + Duration::new(seconds as u64, nanos))
    } else {
        let duration = Duration::new(seconds.unsigned_abs(), nanos);
        UNIX_EPOCH.checked_sub(duration)
    }
}

fn parse_datetime_with_optional_offset(value: &str) -> Option<SystemTime> {
    let (date_part, rest) = if let Some(index) = value.find('T') {
        value.split_at(index)
    } else if let Some(index) = value.find(' ') {
        value.split_at(index)
    } else {
        return None;
    };

    let time_and_offset = rest.get(1..)?.trim();
    if time_and_offset.is_empty() {
        return None;
    }

    let (year, month, day) = parse_date_part(date_part)?;
    let (time_part, offset_seconds) = split_time_and_offset(time_and_offset)?;
    let (hour, minute, second) = parse_time_part(time_part)?;
    let days = days_from_civil(year, month, day);
    let unix_seconds = days
        .checked_mul(86_400)?
        .checked_add((hour as i64) * 3_600)?
        .checked_add((minute as i64) * 60)?
        .checked_add(second as i64)?
        .checked_sub(offset_seconds as i64)?;

    timestamp_from_integer(unix_seconds)
}

fn split_time_and_offset(value: &str) -> Option<(&str, i32)> {
    if let Some(stripped) = value.strip_suffix('Z').or_else(|| value.strip_suffix('z')) {
        return Some((stripped, 0));
    }

    for separator in ['+', '-'] {
        if let Some(index) = value.rfind(separator) {
            let offset = parse_offset_seconds(&value[index..])?;
            return Some((&value[..index], offset));
        }
    }

    Some((value, 0))
}

fn parse_date_part(value: &str) -> Option<(i32, u32, u32)> {
    let mut parts = value.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    Some((year, month, day))
}

fn parse_time_part(value: &str) -> Option<(u32, u32, u32)> {
    let time = value
        .split_once('.')
        .map(|(time, _)| time)
        .unwrap_or(value)
        .trim();
    let mut parts = time.split(':');
    let hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next()?.parse::<u32>().ok()?;
    let second = match parts.next() {
        Some(value) => value.parse::<u32>().ok()?,
        None => 0,
    };
    Some((hour, minute, second))
}

fn parse_offset_seconds(value: &str) -> Option<i32> {
    let sign = match value.chars().next()? {
        '+' => 1_i32,
        '-' => -1_i32,
        _ => return None,
    };
    let rest = &value[1..];
    let (hours, minutes) = if let Some((hours, minutes)) = rest.split_once(':') {
        (hours, minutes)
    } else if rest.len() == 4 {
        (&rest[..2], &rest[2..])
    } else {
        return None;
    };
    let hours = hours.parse::<i32>().ok()?;
    let minutes = minutes.parse::<i32>().ok()?;
    Some(sign * (hours * 3_600 + minutes * 60))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - if month <= 2 { 1 } else { 0 };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    (era as i64) * 146_097 + day_of_era as i64 - 719_468
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era as i32 + era as i32 * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };

    (year, month as u32, day as u32)
}

fn format_optional_timestamp(timestamp: Option<SystemTime>) -> String {
    timestamp
        .map(format_timestamp_label)
        .unwrap_or_else(|| PLACEHOLDER_LABEL.to_string())
}

fn format_timestamp_label(timestamp: SystemTime) -> String {
    let seconds = match timestamp.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(error) => -(error.duration().as_secs() as i64),
    };
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

fn is_within_window(timestamp: SystemTime, now: SystemTime, window: Duration) -> bool {
    now.duration_since(timestamp)
        .map(|duration| duration <= window)
        .unwrap_or(true)
}

fn get_u64_field(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(parse_u64_value)
}

fn get_u8_field(value: &Value, key: &str) -> Option<u8> {
    value
        .get(key)
        .and_then(parse_f64_value)
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= u8::MAX as f64)
        .map(|value| value.round() as u8)
}

fn get_string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn get_string_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cursor = value;

    for segment in path {
        cursor = cursor.get(*segment)?;
    }

    cursor
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_u64_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
                .map(|value| value as u64)
        }),
        Value::String(string) => {
            let trimmed = string.trim();
            trimmed.parse::<u64>().ok().or_else(|| {
                trimmed
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
                    .map(|value| value as u64)
            })
        }
        _ => None,
    }
}

fn parse_f64_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) => string.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn find_plan_type_in_value(value: &Value) -> Option<String> {
    AUTH_PLAN_PATHS
        .iter()
        .find_map(|path| get_string_at_path(value, path).and_then(normalize_plan_type))
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let mut segments = token.split('.');
    let _header = segments.next()?;
    let payload = segments.next()?;
    let decoded = decode_base64url(payload)?;
    serde_json::from_slice(&decoded).ok()
}

fn decode_base64url(value: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity((value.len() * 3) / 4 + 3);
    let mut buffer = 0_u32;
    let mut bits = 0_u32;

    for byte in value.bytes() {
        let sextet = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => break,
            _ if byte.is_ascii_whitespace() => continue,
            _ => return None,
        } as u32;

        buffer = (buffer << 6) | sextet;
        bits += 6;

        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    if bits > 0 && (buffer & ((1_u32 << bits) - 1)) != 0 {
        return None;
    }

    Some(output)
}

fn normalize_plan_type(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn add_token_breakdown(target: &mut TokenBreakdown, value: &TokenBreakdown) {
    target.total_tokens += value.total_tokens;
    add_optional_u64(&mut target.input_tokens, value.input_tokens);
    add_optional_u64(&mut target.output_tokens, value.output_tokens);
    add_optional_u64(
        &mut target.cache_creation_input_tokens,
        value.cache_creation_input_tokens,
    );
    add_optional_u64(
        &mut target.cache_read_input_tokens,
        value.cache_read_input_tokens,
    );
}

fn add_optional_u64(target: &mut Option<u64>, value: Option<u64>) {
    let Some(value) = value else {
        return;
    };

    *target = Some(target.unwrap_or(0) + value);
}

fn finalize_breakdown(value: &mut TokenBreakdown) {
    value.input_tokens.get_or_insert(0);
    value.output_tokens.get_or_insert(0);
    value.cache_creation_input_tokens.get_or_insert(0);
    value.cache_read_input_tokens.get_or_insert(0);
}

fn default_primary_rate_limit() -> RateLimitWindow {
    RateLimitWindow {
        label: PRIMARY_WINDOW_LABEL,
        used_percent: None,
        resets_at: PLACEHOLDER_LABEL.to_string(),
    }
}

fn default_secondary_rate_limit() -> RateLimitWindow {
    RateLimitWindow {
        label: SECONDARY_WINDOW_LABEL,
        used_percent: None,
        resets_at: PLACEHOLDER_LABEL.to_string(),
    }
}

fn unavailable_claude_summary() -> ClaudeUsageSummary {
    ClaudeUsageSummary {
        availability: UsageAvailability::Unavailable,
        source_label: UNAVAILABLE_LABEL.to_string(),
        last_active_at: PLACEHOLDER_LABEL.to_string(),
        last_24h: TokenBreakdown::default(),
        last_7d: TokenBreakdown::default(),
        lifetime: TokenBreakdown::default(),
        hint: CLAUDE_HINT.to_string(),
    }
}

fn unavailable_codex_summary() -> CodexUsageSummary {
    CodexUsageSummary {
        availability: UsageAvailability::Unavailable,
        plan_type: UNKNOWN_PLAN_LABEL.to_string(),
        last_active_at: PLACEHOLDER_LABEL.to_string(),
        last_session_total_tokens: None,
        last_24h_total_tokens: None,
        last_7d_total_tokens: None,
        primary_rate_limit: default_primary_rate_limit(),
        secondary_rate_limit: default_secondary_rate_limit(),
        hint: CODEX_HINT.to_string(),
    }
}

fn update_latest_time(target: &mut Option<SystemTime>, candidate: Option<SystemTime>) {
    let Some(candidate) = candidate else {
        return;
    };

    if target.as_ref().is_none_or(|current| candidate > *current) {
        *target = Some(candidate);
    }
}

fn update_optional_u64(
    target: &mut Option<u64>,
    target_at: &mut Option<SystemTime>,
    candidate_at: Option<SystemTime>,
    candidate_value: u64,
) {
    if target_at.is_none() || candidate_at > *target_at {
        *target = Some(candidate_value);
        *target_at = candidate_at;
    }
}

fn update_optional_string(
    target: &mut Option<String>,
    target_at: &mut Option<SystemTime>,
    candidate_at: Option<SystemTime>,
    candidate_value: String,
) {
    if target_at.is_none() || candidate_at > *target_at {
        *target = Some(candidate_value);
        *target_at = candidate_at;
    }
}

fn update_optional_snapshot(
    target: &mut Option<RateLimitWindow>,
    target_at: &mut Option<SystemTime>,
    candidate_at: Option<SystemTime>,
    candidate_value: RateLimitWindow,
) {
    if target_at.is_none() || candidate_at > *target_at {
        *target = Some(candidate_value);
        *target_at = candidate_at;
    }
}

fn append_line_warning(warnings: &mut Vec<String>, provider: &str, count: usize, detail: &str) {
    if count == 0 {
        return;
    }

    let detail = if count == 1 {
        detail.trim_end_matches('s')
    } else {
        detail
    };

    warnings.push(format!("{provider} skipped {count} {detail}"));
}

#[derive(Default)]
struct ClaudeAggregate {
    has_jsonl_files: bool,
    usage_records: usize,
    latest_activity_at: Option<SystemTime>,
    last_24h: TokenBreakdown,
    last_7d: TokenBreakdown,
    lifetime: TokenBreakdown,
    malformed_lines: usize,
    unreadable_files: usize,
}

#[derive(Default)]
struct CodexAggregate {
    has_sessions_dir: bool,
    has_session_files: bool,
    token_records: usize,
    latest_activity_at: Option<SystemTime>,
    last_session_total_tokens: Option<u64>,
    last_session_total_at: Option<SystemTime>,
    last_24h_total_tokens: u64,
    last_7d_total_tokens: u64,
    primary_rate_limit: Option<RateLimitWindow>,
    primary_rate_limit_at: Option<SystemTime>,
    secondary_rate_limit: Option<RateLimitWindow>,
    secondary_rate_limit_at: Option<SystemTime>,
    plan_from_session: Option<String>,
    plan_from_session_at: Option<SystemTime>,
    malformed_lines: usize,
    unreadable_files: usize,
}

struct RateLimitSnapshot {
    plan_type: Option<String>,
    primary_rate_limit: RateLimitWindow,
    secondary_rate_limit: RateLimitWindow,
}

struct FileProcessError {
    count_as_unreadable_file: bool,
}

impl FileProcessError {
    fn unreadable() -> Self {
        Self {
            count_as_unreadable_file: true,
        }
    }
}

enum AuthPlanRead {
    Found(String),
    Missing,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::model::UsageAvailability;

    use super::{
        collect_ai_usage_summary_with_home, format_timestamp_label, parse_timestamp_string,
    };

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "pup-cli-start-rust-{prefix}-{}-{timestamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn write_file(&self, relative_path: &str, content: &str) {
            let path = self.path.join(relative_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn aggregates_claude_and_codex_usage_from_fixtures() {
        let home = TestDir::new("ai-usage-aggregate");
        home.write_file(
            ".claude/projects/project-a.jsonl",
            fixture_text("claude-usage/usage-ok.jsonl"),
        );
        home.write_file(
            ".codex/sessions/2026/03/28/session-a.jsonl",
            fixture_text("codex-usage/session-last-token.jsonl"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.claude.availability, UsageAvailability::Live);
        assert_eq!(summary.claude.source_label, "local jsonl aggregate");
        assert_eq!(
            summary.claude.last_active_at,
            format_timestamp_label(UNIX_EPOCH + Duration::from_secs(1_799_999_700))
        );
        assert_eq!(summary.claude.last_24h.total_tokens, 225);
        assert_eq!(summary.claude.last_7d.total_tokens, 265);
        assert_eq!(summary.claude.lifetime.total_tokens, 295);

        assert_eq!(summary.codex.availability, UsageAvailability::Live);
        assert_eq!(summary.codex.plan_type, "pro");
        assert_eq!(
            summary.codex.last_active_at,
            format_timestamp_label(UNIX_EPOCH + Duration::from_secs(1_799_999_900))
        );
        assert_eq!(summary.codex.last_session_total_tokens, Some(200));
        assert_eq!(summary.codex.last_24h_total_tokens, Some(90));
        assert_eq!(summary.codex.last_7d_total_tokens, Some(140));
        assert_eq!(summary.codex.primary_rate_limit.used_percent, Some(42));
        assert!(summary.warnings.is_empty());
    }

    #[test]
    fn skips_malformed_claude_lines_without_failing_the_summary() {
        let home = TestDir::new("ai-usage-claude-malformed");
        home.write_file(
            ".claude/projects/project-a.jsonl",
            fixture_text("claude-usage/usage-malformed.jsonl"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.claude.availability, UsageAvailability::Live);
        assert_eq!(summary.claude.last_24h.total_tokens, 60);
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning == "Claude skipped 1 malformed line")
        );
    }

    #[test]
    fn keeps_claude_placeholder_rows_when_no_usage_exists() {
        let home = TestDir::new("ai-usage-claude-empty");
        home.write_file(
            ".claude/projects/project-a.jsonl",
            fixture_text("claude-usage/no-usage.jsonl"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.claude.availability, UsageAvailability::Partial);
        assert_eq!(summary.claude.last_24h.total_tokens, 0);
        assert_eq!(summary.claude.last_24h.input_tokens, None);
        assert_eq!(
            summary.claude.hint,
            "Run /usage or /stats in Claude for plan limits"
        );
    }

    #[test]
    fn uses_last_token_usage_for_codex_window_aggregation() {
        let home = TestDir::new("ai-usage-codex-last-token");
        home.write_file(
            ".codex/sessions/2026/03/28/session-a.jsonl",
            fixture_text("codex-usage/session-last-token.jsonl"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.codex.last_session_total_tokens, Some(200));
        assert_eq!(summary.codex.last_24h_total_tokens, Some(90));
        assert_eq!(summary.codex.last_7d_total_tokens, Some(140));
    }

    #[test]
    fn falls_back_to_total_delta_when_last_token_usage_is_missing() {
        let home = TestDir::new("ai-usage-codex-delta");
        home.write_file(
            ".codex/sessions/2026/03/28/session-a.jsonl",
            fixture_text("codex-usage/session-delta-fallback.jsonl"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.codex.last_session_total_tokens, Some(160));
        assert_eq!(summary.codex.last_24h_total_tokens, Some(60));
        assert_eq!(summary.codex.last_7d_total_tokens, Some(160));
    }

    #[test]
    fn keeps_codex_session_totals_when_rate_limits_are_missing() {
        let home = TestDir::new("ai-usage-codex-no-rate-limits");
        home.write_file(
            ".codex/sessions/2026/03/28/session-a.jsonl",
            fixture_text("codex-usage/session-no-rate-limit.jsonl"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.codex.availability, UsageAvailability::Live);
        assert_eq!(summary.codex.last_session_total_tokens, Some(70));
        assert_eq!(summary.codex.primary_rate_limit.used_percent, None);
        assert_eq!(summary.codex.secondary_rate_limit.used_percent, None);
    }

    #[test]
    fn reads_latest_codex_event_msg_schema() {
        let home = TestDir::new("ai-usage-codex-event-msg");
        home.write_file(
            ".codex/sessions/2026/03/28/session-a.jsonl",
            fixture_text("codex-usage/session-event-msg.jsonl"),
        );

        let now = parse_timestamp_string("2026-03-28T15:00:00Z").unwrap();
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.codex.availability, UsageAvailability::Live);
        assert_eq!(summary.codex.plan_type, "plus");
        assert_eq!(summary.codex.last_session_total_tokens, Some(95_328));
        assert_eq!(summary.codex.last_24h_total_tokens, Some(95_328));
        assert_eq!(summary.codex.last_7d_total_tokens, Some(95_328));
        assert_eq!(summary.codex.primary_rate_limit.used_percent, Some(13));
        assert_eq!(summary.codex.secondary_rate_limit.used_percent, Some(56));
        assert!(summary.warnings.is_empty());
    }

    #[test]
    fn reads_codex_plan_type_from_jwt_id_token_claims() {
        let home = TestDir::new("ai-usage-codex-jwt-auth");
        home.write_file(
            ".codex/sessions/2026/03/28/session-a.jsonl",
            fixture_text("codex-usage/session-no-rate-limit.jsonl"),
        );
        home.write_file(
            ".codex/auth.json",
            fixture_text("codex-usage/auth-jwt-plan.json"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.codex.plan_type, "plus");
        assert!(
            !summary
                .warnings
                .iter()
                .any(|warning| warning == "Codex auth plan type unavailable")
        );
    }

    #[test]
    fn falls_back_to_unknown_when_auth_plan_type_is_not_whitelisted() {
        let home = TestDir::new("ai-usage-codex-auth");
        home.write_file(
            ".codex/sessions/2026/03/28/session-a.jsonl",
            fixture_text("codex-usage/session-no-rate-limit.jsonl"),
        );
        home.write_file(
            ".codex/auth.json",
            fixture_text("codex-usage/auth-unsafe.json"),
        );

        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let summary = collect_ai_usage_summary_with_home(Some(home.path.as_path()), now);

        assert_eq!(summary.codex.plan_type, "unknown");
        assert!(
            summary
                .warnings
                .iter()
                .any(|warning| warning == "Codex auth plan type unavailable")
        );
    }

    #[test]
    fn parses_rfc3339_offsets_for_rate_limit_timestamps() {
        let parsed = parse_timestamp_string("2026-03-28T12:34:56+08:00").unwrap();
        assert_eq!(format_timestamp_label(parsed), "2026-03-28 04:34 UTC");
    }

    fn fixture_text(name: &str) -> &'static str {
        match name {
            "claude-usage/usage-ok.jsonl" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/claude-usage/usage-ok.jsonl"
            )),
            "claude-usage/usage-malformed.jsonl" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/claude-usage/usage-malformed.jsonl"
            )),
            "claude-usage/no-usage.jsonl" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/claude-usage/no-usage.jsonl"
            )),
            "codex-usage/session-last-token.jsonl" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-usage/session-last-token.jsonl"
            )),
            "codex-usage/session-delta-fallback.jsonl" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-usage/session-delta-fallback.jsonl"
            )),
            "codex-usage/session-no-rate-limit.jsonl" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-usage/session-no-rate-limit.jsonl"
            )),
            "codex-usage/session-event-msg.jsonl" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-usage/session-event-msg.jsonl"
            )),
            "codex-usage/auth-unsafe.json" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-usage/auth-unsafe.json"
            )),
            "codex-usage/auth-jwt-plan.json" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex-usage/auth-jwt-plan.json"
            )),
            other => panic!("unknown fixture: {other}"),
        }
    }
}
