use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::model::{ServiceEntry, ServiceManager, ServiceStatus};

pub fn collect_services() -> Vec<ServiceEntry> {
    let entries = match std::env::consts::OS {
        "macos" => collect_macos_services(),
        "linux" => collect_linux_services(),
        _ => Vec::new(),
    };

    if entries.is_empty() {
        vec![placeholder_service_entry()]
    } else {
        entries
    }
}

fn collect_macos_services() -> Vec<ServiceEntry> {
    run_command("brew", &["services", "list"])
        .map(|output| parse_brew_services(&output))
        .unwrap_or_default()
}

fn collect_linux_services() -> Vec<ServiceEntry> {
    if Path::new("/run/systemd/system").exists() {
        let units = run_command(
            "systemctl",
            &[
                "list-units",
                "--type=service",
                "--all",
                "--no-legend",
                "--no-pager",
            ],
        )
        .unwrap_or_default();
        let unit_files = run_command(
            "systemctl",
            &[
                "list-unit-files",
                "--type=service",
                "--no-legend",
                "--no-pager",
            ],
        )
        .unwrap_or_default();

        let entries = parse_systemd_services(&units, &unit_files);
        if !entries.is_empty() {
            return entries;
        }
    }

    run_command("service", &["--status-all"])
        .map(|output| parse_sysv_services(&output))
        .unwrap_or_default()
}

fn parse_brew_services(output: &str) -> Vec<ServiceEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Name") {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let name = parts[0].to_string();
        let detail = parts[1].to_string();
        entries.push(ServiceEntry {
            name,
            manager: ServiceManager::Brew,
            status: classify_service_status(&detail),
            detail,
        });
    }

    sort_services(&mut entries);
    entries
}

fn parse_systemd_services(units_output: &str, unit_files_output: &str) -> Vec<ServiceEntry> {
    let mut runtime_states = BTreeMap::new();
    let mut unit_file_states = BTreeMap::new();

    for raw in units_output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let unit = parts[0];
        let active = parts[2];
        let sub = parts[3];

        if !is_service_unit(unit) {
            continue;
        }

        runtime_states.insert(unit.to_string(), format!("{active}/{sub}"));
    }

    for raw in unit_files_output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(unit) = parts.next() else {
            continue;
        };

        if !is_service_unit(unit) {
            continue;
        }

        let remainder = parts.collect::<Vec<_>>().join(" ");
        if remainder.is_empty() {
            continue;
        }

        unit_file_states.insert(unit.to_string(), remainder);
    }

    let preferred_states = [
        "enabled",
        "disabled",
        "masked",
        "generated",
        "linked",
        "linked-runtime",
    ];

    let units = runtime_states
        .keys()
        .chain(unit_file_states.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut entries = Vec::new();

    for unit in units {
        let runtime = runtime_states.get(&unit).cloned();
        let file_state = unit_file_states.get(&unit).cloned();

        let first_file_state = file_state
            .as_deref()
            .and_then(|value| value.split_whitespace().next())
            .unwrap_or("-");
        let should_include =
            preferred_states.contains(&first_file_state) || runtime_states.contains_key(&unit);
        if !should_include {
            continue;
        }

        let detail = match (runtime, file_state) {
            (Some(runtime), Some(file_state)) => format!("{runtime} / {file_state}"),
            (Some(runtime), None) => runtime,
            (None, Some(file_state)) => file_state,
            (None, None) => "-".to_string(),
        };

        entries.push(ServiceEntry {
            name: clean_service_name(&unit),
            manager: ServiceManager::Systemd,
            status: classify_service_status(&detail),
            detail,
        });
    }

    sort_services(&mut entries);
    entries
}

fn parse_sysv_services(output: &str) -> Vec<ServiceEntry> {
    let mut entries = Vec::new();

    for raw in output.lines() {
        let line = raw.trim();
        if !line.starts_with('[') {
            continue;
        }

        let Some(end_bracket) = line.find(']') else {
            continue;
        };

        let status_code = line[1..end_bracket].trim();
        let name = line[end_bracket + 1..].trim();
        if name.is_empty() {
            continue;
        }

        let detail = match status_code {
            "+" => "running",
            "-" => "stopped",
            _ => "unknown",
        }
        .to_string();

        entries.push(ServiceEntry {
            name: name.to_string(),
            manager: ServiceManager::SysV,
            status: classify_service_status(&detail),
            detail,
        });
    }

    sort_services(&mut entries);
    entries
}

fn clean_service_name(unit: &str) -> String {
    unit.strip_suffix(".service").unwrap_or(unit).to_string()
}

fn is_service_unit(unit: &str) -> bool {
    unit.ends_with(".service") && !unit.ends_with("@.service")
}

fn placeholder_service_entry() -> ServiceEntry {
    ServiceEntry {
        name: "services".to_string(),
        manager: ServiceManager::Unknown,
        status: ServiceStatus::Unknown,
        detail: "-".to_string(),
    }
}

fn sort_services(entries: &mut [ServiceEntry]) {
    entries.sort_by(|left, right| {
        service_status_rank(&left.status)
            .cmp(&service_status_rank(&right.status))
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
    });
}

fn service_status_rank(status: &ServiceStatus) -> usize {
    match status {
        ServiceStatus::Running => 0,
        ServiceStatus::Stopped => 1,
        ServiceStatus::Error => 2,
        ServiceStatus::Unknown => 3,
    }
}

fn classify_service_status(detail: &str) -> ServiceStatus {
    let value = detail.to_ascii_lowercase();

    if value.contains("failed") || value.contains("error") {
        ServiceStatus::Error
    } else if value.contains("inactive") || value.contains("stopped") || value.contains("disabled")
    {
        ServiceStatus::Stopped
    } else if value.contains("running") || value.contains("started") || value.contains("active") {
        ServiceStatus::Running
    } else {
        ServiceStatus::Unknown
    }
}

fn run_command(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success()
        && output.status.code() != Some(1)
        && output.status.code() != Some(3)
    {
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
    use super::{parse_brew_services, parse_systemd_services, parse_sysv_services};
    use crate::model::ServiceStatus;

    #[test]
    fn parses_brew_services_rows() {
        let output = "\
Name Status User File
postgresql@16 started liupx ~/Library/LaunchAgents/homebrew.mxcl.postgresql@16.plist
redis stopped
";

        let entries = parse_brew_services(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "postgresql@16");
        assert_eq!(entries[0].status, ServiceStatus::Running);
        assert_eq!(entries[1].name, "redis");
        assert_eq!(entries[1].status, ServiceStatus::Stopped);
    }

    #[test]
    fn parses_systemd_units_and_unit_files() {
        let units = "\
cron.service loaded active running Regular background program processing daemon
gpu-manager.service loaded inactive dead Detect the available GPUs
failed-demo.service loaded failed failed Broken demo
";
        let unit_files = "\
cron.service enabled enabled
gpu-manager.service enabled enabled
failed-demo.service enabled enabled
";

        let entries = parse_systemd_services(units, unit_files);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "cron");
        assert_eq!(entries[0].status, ServiceStatus::Running);
        assert_eq!(entries[1].name, "gpu-manager");
        assert_eq!(entries[1].status, ServiceStatus::Stopped);
        assert_eq!(entries[2].name, "failed-demo");
        assert_eq!(entries[2].status, ServiceStatus::Error);
    }

    #[test]
    fn parses_sysv_status_lines() {
        let output = "\
[ + ]  ssh
[ - ]  cron
[ ? ]  mystery
";

        let entries = parse_sysv_services(output);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "ssh");
        assert_eq!(entries[0].status, ServiceStatus::Running);
        assert_eq!(entries[1].name, "cron");
        assert_eq!(entries[1].status, ServiceStatus::Stopped);
        assert_eq!(entries[2].name, "mystery");
        assert_eq!(entries[2].status, ServiceStatus::Unknown);
    }
}
