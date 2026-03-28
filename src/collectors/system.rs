use std::env;
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::time::Duration;

use crate::collectors::ai_tools::collect_ai_tools_summary;
use crate::model::{SystemSummary, WelcomeSnapshot};
use crate::shell;

pub fn collect_system_summary() -> SystemSummary {
    SystemSummary {
        os_label: detect_os_label(),
        load_label: detect_load_label(),
        host_label: detect_hostname(),
        disk_label: detect_disk_label(),
        cpu_label: detect_cpu_label(),
        shell_label: shell::current_shell_label(),
        memory_label: detect_memory_label(),
        proxy_label: detect_proxy_label(),
        uptime_label: detect_uptime_label(),
        time_label: detect_time_label(),
    }
}

pub fn collect_welcome_snapshot() -> WelcomeSnapshot {
    let system = collect_system_summary();
    let ai_tools = collect_ai_tools_summary();

    WelcomeSnapshot {
        timestamp: system.time_label.clone(),
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

fn detect_proxy_label() -> String {
    let address: SocketAddr = match "127.0.0.1:7892".parse() {
        Ok(value) => value,
        Err(_) => return "未启用".to_string(),
    };

    if TcpStream::connect_timeout(&address, Duration::from_millis(120)).is_ok() {
        return "已启用 (TUN)".to_string();
    }

    let proxy = [
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ]
    .iter()
    .find_map(|key| env::var(key).ok())
    .unwrap_or_default();

    if proxy.is_empty() || proxy == "off" || proxy == "none" {
        "未启用".to_string()
    } else {
        "已启用".to_string()
    }
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

fn make_usage_bar(percent: usize, slots: usize) -> String {
    if slots == 0 {
        return String::new();
    }

    let filled = ((percent.saturating_mul(slots) + 99) / 100).min(slots);
    let empty = slots.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
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
