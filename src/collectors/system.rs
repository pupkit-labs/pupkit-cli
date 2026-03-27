use std::env;
use std::process::Command;

use crate::model::SystemSummary;
use crate::shell;

pub fn collect_system_summary() -> SystemSummary {
    SystemSummary {
        host: detect_hostname(),
        os_label: detect_os_label(),
        arch: env::consts::ARCH.to_string(),
        shell_label: shell::current_shell_label(),
        project_stage: "bootstrap skeleton ready".to_string(),
    }
}

fn detect_hostname() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| run_command("hostname", &[]))
        .unwrap_or_else(|| "unknown-host".to_string())
}

fn detect_os_label() -> String {
    match env::consts::OS {
        "macos" => "macOS".to_string(),
        "linux" => "Linux".to_string(),
        "windows" => "Windows".to_string(),
        other => other.to_string(),
    }
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
