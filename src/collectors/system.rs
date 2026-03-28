use std::env;
use std::fs;
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::collectors::ai_tools::collect_ai_tools_summary;
use crate::model::{PublicIpSource, PublicIpSummary, SystemSummary, WelcomeSnapshot};
use crate::shell;

const PUBLIC_IP_CACHE_DIR: &str = ".cache/liupx_welcome";
const PUBLIC_IP_CACHE_FILE: &str = "pup_public_ip.json";
const PUBLIC_IP_LEGACY_CACHE_FILE: &str = "ip_info.json";
const PUBLIC_IP_CACHE_TTL_SECS: u64 = 300;
const PUBLIC_IP_CONNECT_TIMEOUT_SECS: &str = "1";
const PUBLIC_IP_TOTAL_TIMEOUT_SECS: &str = "2";
const PUBLIC_IPINFO_JSON_URL: &str = "https://ipinfo.io/json";
const PUBLIC_IPINFO_COUNTRY_URL: &str = "https://ipinfo.io/country";
const PUBLIC_IPINFO_IP_URL: &str = "https://ipinfo.io/ip";
const ICANHAZIP_URL: &str = "https://icanhazip.com";
const PROXY_TUN_ADDR_ENV: &str = "PUP_PROXY_TUN_ADDR";
const PROXY_ENV_KEYS: [&str; 6] = [
    "http_proxy",
    "HTTP_PROXY",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
];

pub fn collect_system_summary() -> SystemSummary {
    SystemSummary {
        os_label: detect_os_label(),
        load_label: detect_load_label(),
        host_label: detect_hostname(),
        disk_label: detect_disk_label(),
        cpu_label: detect_cpu_label(),
        shell_label: shell::current_shell_label(),
        memory_label: detect_memory_label(),
        public_ip: collect_public_ip_summary(),
        proxy_label: detect_proxy_label(),
        uptime_label: detect_uptime_label(),
    }
}

pub fn collect_welcome_snapshot() -> WelcomeSnapshot {
    let system = collect_system_summary();
    let ai_tools = collect_ai_tools_summary();

    WelcomeSnapshot {
        timestamp: detect_time_label(),
        user_label: detect_user_label(),
        host_label: system.host_label.clone(),
        current_dir: detect_current_dir(),
        system,
        ai_tools,
    }
}

fn detect_hostname() -> String {
    run_command("hostname", &["-s"])
        .or_else(|| {
            env::var("HOSTNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| run_command("hostname", &[]))
        .unwrap_or_else(|| "-".to_string())
}

fn detect_os_label() -> String {
    let arch = detect_arch();

    match env::consts::OS {
        "macos" => {
            let name =
                run_command("sw_vers", &["-productName"]).unwrap_or_else(|| "macOS".to_string());
            let version = run_command("sw_vers", &["-productVersion"]).unwrap_or_default();

            if version.is_empty() {
                format!("{name} ({arch})")
            } else {
                format!("{name} {version} ({arch})")
            }
        }
        "linux" => {
            let pretty = detect_linux_pretty_name()
                .or_else(|| run_command("uname", &["-sr"]))
                .unwrap_or_else(|| "Linux".to_string());
            format!("{pretty} ({arch})")
        }
        other => {
            let base = run_command("uname", &["-sr"]).unwrap_or_else(|| other.to_string());
            format!("{base} ({arch})")
        }
    }
}

fn detect_cpu_label() -> String {
    match env::consts::OS {
        "macos" => run_command("sysctl", &["-n", "machdep.cpu.brand_string"])
            .or_else(|| {
                if detect_arch() == "arm64" {
                    Some("Apple Silicon (arm64)".to_string())
                } else {
                    None
                }
            })
            .or_else(|| run_command("sysctl", &["-n", "hw.model"]))
            .unwrap_or_else(detect_arch),
        "linux" => read_linux_cpu_model().unwrap_or_else(detect_arch),
        _ => detect_arch(),
    }
}

fn detect_memory_label() -> String {
    match env::consts::OS {
        "macos" => run_command("sysctl", &["-n", "hw.memsize"])
            .and_then(|value| value.parse::<u64>().ok())
            .map(|bytes| format!("{} total", format_bytes(bytes)))
            .or_else(detect_macos_hostinfo_memory)
            .unwrap_or_else(|| "-".to_string()),
        "linux" => detect_linux_memory_label().unwrap_or_else(|| "-".to_string()),
        _ => "-".to_string(),
    }
}

fn detect_uptime_label() -> String {
    let raw = match run_command("uptime", &[]) {
        Some(value) => value,
        None => return "-".to_string(),
    };

    if !raw.contains(" up ") {
        return "-".to_string();
    }

    let trimmed = raw
        .split_once(" up ")
        .map(|(_, tail)| tail.to_string())
        .unwrap_or(raw);

    trimmed
        .split("load averages:")
        .next()
        .and_then(|value| value.split("load average:").next())
        .map(|value| value.trim().trim_end_matches(',').trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_string())
}

fn detect_load_label() -> String {
    let raw = match run_command("uptime", &[]) {
        Some(value) => value,
        None => return "-".to_string(),
    };

    let load = if let Some((_, value)) = raw.split_once("load averages:") {
        value.trim().to_string()
    } else if let Some((_, value)) = raw.split_once("load average:") {
        value.trim().to_string()
    } else {
        return "-".to_string();
    };

    let normalized = load.replace(',', " ");
    let parts: Vec<&str> = normalized.split_whitespace().collect();

    if parts.len() >= 3 {
        format!("1分 {} · 5分 {} · 15分 {}", parts[0], parts[1], parts[2])
    } else {
        load
    }
}

fn detect_disk_label() -> String {
    let output = match run_command("df", &["-h", "/"]) {
        Some(value) => value,
        None => return "-".to_string(),
    };

    let line = match output.lines().nth(1) {
        Some(value) => value,
        None => return "-".to_string(),
    };

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return "-".to_string();
    }

    let size = parts[1];
    let used = parts[2];
    let percent = parts[4].trim_end_matches('%');
    let usage = percent.parse::<usize>().ok();
    let bar = usage
        .map(|value| make_usage_bar(value, 10))
        .unwrap_or_else(|| "░░░░░░░░░░".to_string());

    format!("{bar} 已用 {used} / 总量 {size} ({percent}%)")
}

fn collect_public_ip_summary() -> PublicIpSummary {
    let home = env::var_os("HOME").map(PathBuf::from);
    collect_public_ip_summary_with_home(
        home.as_deref(),
        current_unix_timestamp_secs(),
        &mut run_command,
    )
}

fn collect_public_ip_summary_with_home(
    home: Option<&Path>,
    now_secs: u64,
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
) -> PublicIpSummary {
    let cache_paths = public_ip_cache_paths(home);
    let cached = cache_paths.as_ref().and_then(load_public_ip_cache);

    if let Some(entry) = cached
        .as_ref()
        .filter(|entry| is_public_ip_cache_fresh(entry, now_secs))
    {
        return entry.to_summary(PublicIpSource::Cache);
    }

    if let Some(entry) = fetch_public_ip_entry(now_secs, runner) {
        if let Some(paths) = cache_paths.as_ref() {
            let _ = write_public_ip_cache(&paths.primary, &entry);
        }

        return entry.to_summary(PublicIpSource::Live);
    }

    cached
        .map(|entry| entry.to_summary(PublicIpSource::Cache))
        .unwrap_or_else(unavailable_public_ip_summary)
}

fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn is_public_ip_cache_fresh(entry: &CachedPublicIp, now_secs: u64) -> bool {
    entry
        .fetched_at
        .is_some_and(|timestamp| now_secs.saturating_sub(timestamp) <= PUBLIC_IP_CACHE_TTL_SECS)
}

fn fetch_public_ip_entry(
    now_secs: u64,
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
) -> Option<CachedPublicIp> {
    let (address, country_label) = if let Some((address, country_label)) = fetch_ipinfo_json(runner)
    {
        (address, country_label)
    } else {
        let address = fetch_plain_public_ip(runner, ICANHAZIP_URL)
            .or_else(|| fetch_plain_public_ip(runner, PUBLIC_IPINFO_IP_URL))?;
        let country_label = fetch_public_ip_country(runner).unwrap_or_default();
        (address, country_label)
    };

    Some(CachedPublicIp {
        fetched_at: Some(now_secs),
        address,
        country_label,
    })
}

fn fetch_ipinfo_json(
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
) -> Option<(String, String)> {
    let body = fetch_url(runner, PUBLIC_IPINFO_JSON_URL)?;
    let address =
        parse_json_string_value(&body, "ip").and_then(|value| normalize_public_ip(&value))?;
    let country_label = parse_json_string_value(&body, "country")
        .or_else(|| parse_json_string_value(&body, "country_name"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();

    Some((address, country_label))
}

fn fetch_plain_public_ip(
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
    url: &str,
) -> Option<String> {
    fetch_url(runner, url).and_then(|value| normalize_public_ip(&value))
}

fn fetch_public_ip_country(
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
) -> Option<String> {
    fetch_url(runner, PUBLIC_IPINFO_COUNTRY_URL)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn fetch_url(
    runner: &mut impl FnMut(&str, &[&str]) -> Option<String>,
    url: &str,
) -> Option<String> {
    runner(
        "curl",
        &[
            "-fsSL",
            "--connect-timeout",
            PUBLIC_IP_CONNECT_TIMEOUT_SECS,
            "--max-time",
            PUBLIC_IP_TOTAL_TIMEOUT_SECS,
            url,
        ],
    )
    .or_else(|| runner("wget", &["-q", "-O", "-", "--tries=1", "--timeout=2", url]))
}

fn unavailable_public_ip_summary() -> PublicIpSummary {
    PublicIpSummary {
        address: "-".to_string(),
        country_label: String::new(),
        source: PublicIpSource::Unavailable,
    }
}

fn public_ip_cache_paths(home: Option<&Path>) -> Option<PublicIpCachePaths> {
    let home = home?;
    let base = home.join(PUBLIC_IP_CACHE_DIR);

    Some(PublicIpCachePaths {
        primary: base.join(PUBLIC_IP_CACHE_FILE),
        legacy: base.join(PUBLIC_IP_LEGACY_CACHE_FILE),
    })
}

fn load_public_ip_cache(paths: &PublicIpCachePaths) -> Option<CachedPublicIp> {
    read_public_ip_cache(&paths.primary).or_else(|| read_public_ip_cache(&paths.legacy))
}

fn read_public_ip_cache(path: &Path) -> Option<CachedPublicIp> {
    let content = fs::read_to_string(path).ok()?;
    let address =
        parse_json_string_value(&content, "ip").and_then(|value| normalize_public_ip(&value))?;
    let country_label = parse_json_string_value(&content, "country")
        .or_else(|| parse_json_string_value(&content, "country_name"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let fetched_at = parse_json_u64_value(&content, "fetched_at").or_else(|| {
        parse_json_string_value(&content, "fetched_at").and_then(|value| value.parse().ok())
    });

    Some(CachedPublicIp {
        fetched_at,
        address,
        country_label,
    })
}

fn write_public_ip_cache(path: &Path, entry: &CachedPublicIp) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let fetched_at = entry.fetched_at.unwrap_or_default();
    let payload = format!(
        "{{\"fetched_at\":{fetched_at},\"ip\":\"{}\",\"country\":\"{}\"}}\n",
        escape_json_string(&entry.address),
        escape_json_string(&entry.country_label)
    );
    fs::write(path, payload)
}

fn normalize_public_ip(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let parsed = trimmed.parse::<IpAddr>().ok()?;
    Some(parsed.to_string())
}

fn detect_proxy_label() -> String {
    let tun_enabled = configured_proxy_tun_addr()
        .map(|address| is_tun_proxy_available(&address))
        .unwrap_or(false);

    classify_proxy_label(tun_enabled, active_proxy_env().as_deref())
}

fn configured_proxy_tun_addr() -> Option<SocketAddr> {
    env::var(PROXY_TUN_ADDR_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
}

fn is_tun_proxy_available(address: &SocketAddr) -> bool {
    TcpStream::connect_timeout(address, Duration::from_millis(120)).is_ok()
}

fn active_proxy_env() -> Option<String> {
    PROXY_ENV_KEYS
        .iter()
        .find_map(|key| env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| is_enabled_proxy_value(value))
}

fn classify_proxy_label(tun_enabled: bool, proxy_env: Option<&str>) -> String {
    if tun_enabled {
        return "已启用 (TUN)".to_string();
    }

    if proxy_env.is_some_and(is_enabled_proxy_value) {
        "已启用 (ENV)".to_string()
    } else {
        "未启用".to_string()
    }
}

fn is_enabled_proxy_value(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !normalized.is_empty() && normalized != "off" && normalized != "none"
}

fn detect_time_label() -> String {
    run_command("date", &["+%Y-%m-%d %H:%M"]).unwrap_or_else(|| "-".to_string())
}

fn detect_user_label() -> String {
    env::var("USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| run_command("whoami", &[]))
        .unwrap_or_else(|| "unknown-user".to_string())
}

fn detect_current_dir() -> String {
    let cwd = match env::current_dir() {
        Ok(value) => value.display().to_string(),
        Err(_) => return "-".to_string(),
    };

    if let Ok(home) = env::var("HOME") {
        if cwd == home {
            return "~".to_string();
        }

        if let Some(suffix) = cwd.strip_prefix(&home) {
            return format!("~{suffix}");
        }
    }

    cwd
}

fn detect_arch() -> String {
    run_command("uname", &["-m"]).unwrap_or_else(|| match env::consts::ARCH {
        "aarch64" => "arm64".to_string(),
        other => other.to_string(),
    })
}

fn detect_linux_pretty_name() -> Option<String> {
    let content = fs::read_to_string("/etc/os-release").ok()?;
    parse_key_value(&content, "PRETTY_NAME")
}

fn read_linux_cpu_model() -> Option<String> {
    let content = fs::read_to_string("/proc/cpuinfo").ok()?;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("model name") || trimmed.starts_with("Hardware") {
            let (_, value) = trimmed.split_once(':')?;
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    None
}

fn detect_macos_hostinfo_memory() -> Option<String> {
    let output = run_command("hostinfo", &[])?;
    for line in output.lines() {
        if let Some((_, value)) = line.split_once("Primary memory available:") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(format!("{value} total"));
            }
        }
    }

    None
}

fn detect_linux_memory_label() -> Option<String> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let total_kib = parse_meminfo_kib(&content, "MemTotal")?;
    let available_kib = parse_meminfo_kib(&content, "MemAvailable")?;
    let used_kib = total_kib.saturating_sub(available_kib);

    Some(format!(
        "{} used / {} total / {} avail",
        format_bytes(used_kib * 1024),
        format_bytes(total_kib * 1024),
        format_bytes(available_kib * 1024)
    ))
}

fn parse_meminfo_kib(content: &str, key: &str) -> Option<u64> {
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with(key) {
            continue;
        }

        let (_, value) = trimmed.split_once(':')?;
        let number = value.split_whitespace().next()?.parse::<u64>().ok()?;
        return Some(number);
    }

    None
}

fn parse_json_string_value(content: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{key}\"");
    let key_start = content.find(&key_pattern)?;
    let rest = &content[key_start + key_pattern.len()..];
    let colon_index = rest.find(':')?;
    parse_quoted_string(rest[colon_index + 1..].trim_start())
}

fn parse_json_u64_value(content: &str, key: &str) -> Option<u64> {
    let key_pattern = format!("\"{key}\"");
    let key_start = content.find(&key_pattern)?;
    let rest = &content[key_start + key_pattern.len()..];
    let colon_index = rest.find(':')?;
    let digits: String = rest[colon_index + 1..]
        .trim_start()
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();

    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn parse_quoted_string(input: &str) -> Option<String> {
    let mut chars = input.chars();
    if chars.next()? != '"' {
        return None;
    }

    let mut output = String::new();

    while let Some(character) = chars.next() {
        match character {
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
                    'u' => {
                        let mut digits = String::new();
                        for _ in 0..4 {
                            digits.push(chars.next()?);
                        }
                        let codepoint = u32::from_str_radix(&digits, 16).ok()?;
                        output.push(char::from_u32(codepoint)?);
                    }
                    other => output.push(other),
                }
            }
            other => output.push(other),
        }
    }

    None
}

fn parse_key_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        let prefix = format!("{key}=");
        if !trimmed.starts_with(&prefix) {
            continue;
        }

        let value = trimmed.trim_start_matches(&prefix).trim_matches('"').trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    None
}

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;

    for (index, unit) in units.iter().enumerate() {
        if size < 1024.0 || index == units.len() - 1 {
            return format!("{size:.1} {unit}");
        }

        size /= 1024.0;
    }

    format!("{size:.1} TiB")
}

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::new();

    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }

    escaped
}

fn make_usage_bar(percent: usize, slots: usize) -> String {
    if slots == 0 {
        return String::new();
    }

    let filled = ((percent.saturating_mul(slots) + 99) / 100).min(slots);
    let empty = slots.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CachedPublicIp {
    fetched_at: Option<u64>,
    address: String,
    country_label: String,
}

impl CachedPublicIp {
    fn to_summary(&self, source: PublicIpSource) -> PublicIpSummary {
        PublicIpSummary {
            address: self.address.clone(),
            country_label: self.country_label.clone(),
            source,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PublicIpCachePaths {
    primary: PathBuf,
    legacy: PathBuf,
}

fn run_command(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::model::PublicIpSource;

    use super::{
        classify_proxy_label, collect_public_ip_summary_with_home, is_enabled_proxy_value,
        parse_meminfo_kib, run_command,
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

        fn read_file(&self, relative_path: &str) -> String {
            std::fs::read_to_string(self.path.join(relative_path)).unwrap()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn proxy_env_values_ignore_disabled_markers() {
        assert!(is_enabled_proxy_value("http://127.0.0.1:7890"));
        assert!(!is_enabled_proxy_value(""));
        assert!(!is_enabled_proxy_value(" off "));
        assert!(!is_enabled_proxy_value("NONE"));
    }

    #[test]
    fn proxy_label_prefers_tun_over_environment_proxy() {
        assert_eq!(
            classify_proxy_label(true, Some("http://127.0.0.1:7890")),
            "已启用 (TUN)"
        );
        assert_eq!(
            classify_proxy_label(false, Some("http://127.0.0.1:7890")),
            "已启用 (ENV)"
        );
        assert_eq!(classify_proxy_label(false, None), "未启用");
    }

    #[test]
    fn meminfo_parser_extracts_requested_key() {
        let content = "MemTotal:       32768000 kB\nMemAvailable:   16384000 kB\n";

        assert_eq!(parse_meminfo_kib(content, "MemTotal"), Some(32_768_000));
        assert_eq!(parse_meminfo_kib(content, "MemAvailable"), Some(16_384_000));
        assert_eq!(parse_meminfo_kib(content, "SwapTotal"), None);
    }

    #[test]
    fn public_ip_uses_fresh_cache_without_running_network_commands() {
        let home = TestDir::new("public-ip-fresh-cache");
        home.write_file(
            ".cache/liupx_welcome/pup_public_ip.json",
            r#"{"fetched_at":950,"ip":"149.28.91.67","country":"United States"}"#,
        );
        let mut calls = 0;
        let mut runner = |_: &str, _: &[&str]| {
            calls += 1;
            None
        };

        let summary =
            collect_public_ip_summary_with_home(Some(home.path.as_path()), 1_000, &mut runner);

        assert_eq!(summary.source, PublicIpSource::Cache);
        assert_eq!(summary.address, "149.28.91.67");
        assert_eq!(summary.country_label, "United States");
        assert_eq!(calls, 0);
    }

    #[test]
    fn public_ip_falls_back_to_stale_cache_when_fetch_fails() {
        let home = TestDir::new("public-ip-stale-cache");
        home.write_file(
            ".cache/liupx_welcome/pup_public_ip.json",
            r#"{"fetched_at":100,"ip":"149.28.91.67","country":"United States"}"#,
        );
        let mut runner = |_: &str, _: &[&str]| None;

        let summary =
            collect_public_ip_summary_with_home(Some(home.path.as_path()), 1_000, &mut runner);

        assert_eq!(summary.source, PublicIpSource::Cache);
        assert_eq!(summary.address, "149.28.91.67");
        assert_eq!(summary.country_label, "United States");
    }

    #[test]
    fn public_ip_reads_legacy_shell_cache_shape() {
        let home = TestDir::new("public-ip-legacy-cache");
        home.write_file(
            ".cache/liupx_welcome/ip_info.json",
            r#"{"ip":"149.28.91.67","country_name":"United States"}"#,
        );
        let mut runner = |_: &str, _: &[&str]| None;

        let summary =
            collect_public_ip_summary_with_home(Some(home.path.as_path()), 1_000, &mut runner);

        assert_eq!(summary.source, PublicIpSource::Cache);
        assert_eq!(summary.address, "149.28.91.67");
        assert_eq!(summary.country_label, "United States");
    }

    #[test]
    fn public_ip_fetches_live_value_and_writes_primary_cache() {
        let home = TestDir::new("public-ip-live");
        let mut runner = |command: &str, args: &[&str]| match (command, args.last().copied()) {
            ("curl", Some("https://ipinfo.io/json")) => {
                Some(r#"{"ip":"149.28.91.67","country":"US"}"#.to_string())
            }
            _ => None,
        };

        let summary =
            collect_public_ip_summary_with_home(Some(home.path.as_path()), 1_000, &mut runner);

        assert_eq!(summary.source, PublicIpSource::Live);
        assert_eq!(summary.address, "149.28.91.67");
        assert_eq!(summary.country_label, "US");

        let cache = home.read_file(".cache/liupx_welcome/pup_public_ip.json");
        assert!(cache.contains("\"fetched_at\":1000"));
        assert!(cache.contains("\"ip\":\"149.28.91.67\""));
        assert!(cache.contains("\"country\":\"US\""));
    }

    #[test]
    fn public_ip_falls_back_to_icanhazip_and_country_endpoint() {
        let home = TestDir::new("public-ip-fallback");
        let mut runner = |command: &str, args: &[&str]| match (command, args.last().copied()) {
            ("curl", Some("https://icanhazip.com")) => Some("149.28.91.67\n".to_string()),
            ("curl", Some("https://ipinfo.io/country")) => Some("US\n".to_string()),
            _ => None,
        };

        let summary =
            collect_public_ip_summary_with_home(Some(home.path.as_path()), 1_000, &mut runner);

        assert_eq!(summary.source, PublicIpSource::Live);
        assert_eq!(summary.address, "149.28.91.67");
        assert_eq!(summary.country_label, "US");
    }

    #[test]
    fn missing_command_returns_none_instead_of_failing() {
        assert!(run_command("pup-command-that-should-not-exist", &[]).is_none());
    }
}
