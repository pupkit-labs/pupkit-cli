use std::env;

use crate::model::{
    AiToolsSummary, AiUsageSummary, RateLimitWindow, ServiceEntry, SystemSummary, TokenBreakdown,
    WelcomeSnapshot,
};

const DEFAULT_WIDTH: usize = 100;
const MIN_LABEL_WIDTH: usize = 4;
const MIN_VALUE_WIDTH: usize = 16;

struct DoubleTableLayout {
    label_width: usize,
    left_value_width: usize,
    right_value_width: usize,
    border_widths: [usize; 4],
}

pub fn render_welcome(snapshot: &WelcomeSnapshot) -> String {
    render_welcome_with_width(snapshot, resolve_total_width())
}

fn render_welcome_with_width(snapshot: &WelcomeSnapshot, total_width: usize) -> String {
    let mut output = String::new();

    output.push('\n');
    output.push_str(" _      ___ _   _ ____  __   __\n");
    output.push_str("| |    |_ _| | | |  _ \\ \\ \\ / /\n");
    output.push_str("| |     | || | | | |_) | \\ V / \n");
    output.push_str("| |___  | || |_| |  __/  / _ \\\n");
    output.push_str("|_____||___|\\___/|_|    /_/ \\_\\\n");
    output.push_str(&format!("Welcome back, {}.\n", snapshot.user_label));
    output.push_str(&format!(
        "{}  {}@{}  {}\n\n",
        snapshot.timestamp, snapshot.user_label, snapshot.host_label, snapshot.current_dir
    ));
    output.push_str(&render_system_summary_with_width(
        &snapshot.system,
        total_width,
    ));
    output.push_str(&render_ai_tools_summary_with_width(
        &snapshot.ai_tools,
        total_width,
    ));

    output
}

pub fn render_system_summary(summary: &SystemSummary) -> String {
    render_system_summary_with_width(summary, resolve_total_width())
}

fn render_system_summary_with_width(summary: &SystemSummary, total_width: usize) -> String {
    let public_ip_label = summary.public_ip.display_label();
    let rows = [
        (
            "OS",
            summary.os_label.as_str(),
            "Load",
            summary.load_label.as_str(),
        ),
        (
            "Host",
            summary.host_label.as_str(),
            "Disk",
            summary.disk_label.as_str(),
        ),
        (
            "CPU",
            summary.cpu_label.as_str(),
            "Shell",
            summary.shell_label.as_str(),
        ),
        (
            "Memory",
            summary.memory_label.as_str(),
            "IP",
            public_ip_label.as_str(),
        ),
        (
            "Uptime",
            summary.uptime_label.as_str(),
            "Proxy",
            summary.proxy_label.as_str(),
        ),
    ];

    render_double_box_table("System Summary", &rows, total_width)
}

pub fn render_ai_tools_summary(summary: &AiToolsSummary) -> String {
    render_ai_tools_summary_with_width(summary, resolve_total_width())
}

pub fn render_ai_usage_summary(summary: &AiUsageSummary) -> String {
    render_ai_usage_summary_with_width(summary, resolve_total_width())
}

pub fn render_services(entries: &[ServiceEntry]) -> String {
    let rows: Vec<(&str, &str)> = entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.detail.as_str()))
        .collect();

    render_box_table("Services", &rows, resolve_total_width())
}

fn render_ai_tools_summary_with_width(summary: &AiToolsSummary, total_width: usize) -> String {
    let rows = [
        (
            "Model",
            summary.claude_model.as_str(),
            "Model",
            summary.codex_model.as_str(),
        ),
        (
            "Skills",
            summary.claude_skills.as_str(),
            "Skills",
            summary.codex_skills.as_str(),
        ),
    ];

    render_grouped_double_box_table("AI Tools", ("Claude", "Codex"), &rows, total_width)
}

fn render_ai_usage_summary_with_width(summary: &AiUsageSummary, total_width: usize) -> String {
    let claude_last_24h = format_token_breakdown(&summary.claude.last_24h);
    let claude_last_7d = format_token_breakdown(&summary.claude.last_7d);
    let claude_lifetime = format_token_breakdown(&summary.claude.lifetime);
    let codex_last_24h = format_optional_total(summary.codex.last_24h_total_tokens);
    let codex_last_7d = format_optional_total(summary.codex.last_7d_total_tokens);
    let codex_session = format_optional_total(summary.codex.last_session_total_tokens);
    let codex_limits = format_rate_limits(summary);

    let rows = [
        (
            "Source",
            summary.claude.source_label.as_str(),
            "Plan",
            summary.codex.plan_type.as_str(),
        ),
        (
            "Last Active",
            summary.claude.last_active_at.as_str(),
            "Last Active",
            summary.codex.last_active_at.as_str(),
        ),
        (
            "24h",
            claude_last_24h.as_str(),
            "24h",
            codex_last_24h.as_str(),
        ),
        ("7d", claude_last_7d.as_str(), "7d", codex_last_7d.as_str()),
        (
            "Lifetime",
            claude_lifetime.as_str(),
            "Session",
            codex_session.as_str(),
        ),
        (
            "Hint",
            summary.claude.hint.as_str(),
            "Limits",
            codex_limits.as_str(),
        ),
    ];

    let mut output =
        render_grouped_double_box_table("AI Usage", ("Claude", "Codex"), &rows, total_width);

    if !summary.warnings.is_empty() {
        let warning_rows: Vec<(String, String)> = summary
            .warnings
            .iter()
            .enumerate()
            .map(|(index, warning)| (format!("{}", index + 1), warning.clone()))
            .collect();
        output.push_str(&render_box_table_owned(
            "Warnings",
            &warning_rows,
            total_width,
        ));
    }

    output
}

fn render_double_box_table(
    title: &str,
    rows: &[(&str, &str, &str, &str)],
    total_width: usize,
) -> String {
    let layout = match resolve_double_table_layout(rows, total_width) {
        Some(layout) => layout,
        None => {
            let single_rows: Vec<(&str, &str)> = rows
                .iter()
                .flat_map(|(left_label, left_value, right_label, right_value)| {
                    [(*left_label, *left_value), (*right_label, *right_value)]
                })
                .collect();
            return render_box_table(title, &single_rows, total_width);
        }
    };
    let mut output = String::new();

    output.push_str(title);
    output.push('\n');
    output.push_str(&render_border("┌", "┬", "┐", &layout.border_widths));
    render_double_box_rows(&mut output, rows, &layout);
    output.push('\n');
    output
}

fn render_grouped_double_box_table(
    title: &str,
    group_headers: (&str, &str),
    rows: &[(&str, &str, &str, &str)],
    total_width: usize,
) -> String {
    let layout = match resolve_double_table_layout(rows, total_width) {
        Some(layout) => layout,
        None => {
            let single_rows: Vec<(String, String)> = rows
                .iter()
                .flat_map(|(left_label, left_value, right_label, right_value)| {
                    [
                        (
                            format!("{} {}", group_headers.0, left_label),
                            (*left_value).to_string(),
                        ),
                        (
                            format!("{} {}", group_headers.1, right_label),
                            (*right_value).to_string(),
                        ),
                    ]
                })
                .collect();
            return render_box_table_owned(title, &single_rows, total_width);
        }
    };
    let grouped_border_widths = [
        layout.border_widths[0] + layout.border_widths[1] + 1,
        layout.border_widths[2] + layout.border_widths[3] + 1,
    ];
    let mut output = String::new();

    output.push_str(title);
    output.push('\n');
    output.push_str(&render_border("┌", "┬", "┐", &grouped_border_widths));
    output.push_str("│ ");
    output.push_str(&pad_visible(group_headers.0, grouped_border_widths[0] - 2));
    output.push_str(" │ ");
    output.push_str(&pad_visible(group_headers.1, grouped_border_widths[1] - 2));
    output.push_str(" │\n");
    output.push_str(&render_grouped_divider(&layout.border_widths));
    render_double_box_rows(&mut output, rows, &layout);
    output.push('\n');
    output
}

fn render_box_table(title: &str, rows: &[(&str, &str)], total_width: usize) -> String {
    let label_width = rows
        .iter()
        .map(|(label, _)| display_width(label))
        .max()
        .unwrap_or(MIN_LABEL_WIDTH)
        .max(MIN_LABEL_WIDTH);
    let value_width = total_width
        .saturating_sub(label_width + 7)
        .max(MIN_VALUE_WIDTH);
    let border_widths = [label_width + 2, value_width + 2];
    let mut output = String::new();

    output.push_str(title);
    output.push('\n');
    output.push_str(&render_border("┌", "┬", "┐", &border_widths));

    for (index, (label, value)) in rows.iter().enumerate() {
        let value_lines = wrap_text(value, value_width);

        for line_index in 0..value_lines.len() {
            let label_text = if line_index == 0 { *label } else { "" };
            let value_text = value_lines
                .get(line_index)
                .map(String::as_str)
                .unwrap_or("");

            output.push_str("│ ");
            output.push_str(&pad_visible(label_text, label_width));
            output.push_str(" │ ");
            output.push_str(&pad_visible(value_text, value_width));
            output.push_str(" │\n");
        }

        let border = if index + 1 == rows.len() {
            render_border("└", "┴", "┘", &border_widths)
        } else {
            render_border("├", "┼", "┤", &border_widths)
        };
        output.push_str(&border);
    }

    output.push('\n');
    output
}

fn render_box_table_owned(title: &str, rows: &[(String, String)], total_width: usize) -> String {
    let borrowed_rows: Vec<(&str, &str)> = rows
        .iter()
        .map(|(label, value)| (label.as_str(), value.as_str()))
        .collect();
    render_box_table(title, &borrowed_rows, total_width)
}

fn resolve_total_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_WIDTH)
}

fn resolve_double_table_layout(
    rows: &[(&str, &str, &str, &str)],
    total_width: usize,
) -> Option<DoubleTableLayout> {
    let label_width = rows
        .iter()
        .flat_map(|(left_label, _, right_label, _)| [*left_label, *right_label])
        .map(display_width)
        .max()
        .unwrap_or(MIN_LABEL_WIDTH)
        .max(MIN_LABEL_WIDTH);

    let available_value_width = total_width.saturating_sub(label_width * 2 + 13);
    if total_width < 70 || available_value_width < MIN_VALUE_WIDTH * 2 {
        return None;
    }

    let left_value_width = available_value_width / 2;
    let right_value_width = available_value_width - left_value_width;
    let border_widths = [
        label_width + 2,
        left_value_width + 2,
        label_width + 2,
        right_value_width + 2,
    ];

    Some(DoubleTableLayout {
        label_width,
        left_value_width,
        right_value_width,
        border_widths,
    })
}

fn render_double_box_rows(
    output: &mut String,
    rows: &[(&str, &str, &str, &str)],
    layout: &DoubleTableLayout,
) {
    for (index, (left_label, left_value, right_label, right_value)) in rows.iter().enumerate() {
        let left_lines = wrap_text(left_value, layout.left_value_width);
        let right_lines = wrap_text(right_value, layout.right_value_width);
        let max_lines = left_lines.len().max(right_lines.len());

        for line_index in 0..max_lines {
            let left_label_text = if line_index == 0 { *left_label } else { "" };
            let right_label_text = if line_index == 0 { *right_label } else { "" };
            let left_value_text = left_lines.get(line_index).map(String::as_str).unwrap_or("");
            let right_value_text = right_lines
                .get(line_index)
                .map(String::as_str)
                .unwrap_or("");

            output.push_str("│ ");
            output.push_str(&pad_visible(left_label_text, layout.label_width));
            output.push_str(" │ ");
            output.push_str(&pad_visible(left_value_text, layout.left_value_width));
            output.push_str(" │ ");
            output.push_str(&pad_visible(right_label_text, layout.label_width));
            output.push_str(" │ ");
            output.push_str(&pad_visible(right_value_text, layout.right_value_width));
            output.push_str(" │\n");
        }

        let border = if index + 1 == rows.len() {
            render_border("└", "┴", "┘", &layout.border_widths)
        } else {
            render_border("├", "┼", "┤", &layout.border_widths)
        };
        output.push_str(&border);
    }
}

fn render_border(left: &str, middle: &str, right: &str, widths: &[usize]) -> String {
    let mut line = String::new();
    line.push_str(left);

    for (index, width) in widths.iter().enumerate() {
        line.push_str(&"─".repeat(*width));
        if index + 1 == widths.len() {
            line.push_str(right);
        } else {
            line.push_str(middle);
        }
    }

    line.push('\n');
    line
}

fn render_grouped_divider(widths: &[usize; 4]) -> String {
    let mut line = String::new();
    line.push('├');
    line.push_str(&"─".repeat(widths[0]));
    line.push('┬');
    line.push_str(&"─".repeat(widths[1]));
    line.push('┼');
    line.push_str(&"─".repeat(widths[2]));
    line.push('┬');
    line.push_str(&"─".repeat(widths[3]));
    line.push('┤');
    line.push('\n');
    line
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    let input = if text.trim().is_empty() { "-" } else { text };
    let mut lines = Vec::new();

    for raw_line in input.lines() {
        if raw_line.trim().is_empty() {
            lines.push(String::new());
            continue;
        }

        let words: Vec<&str> = raw_line.split_whitespace().collect();
        let mut current = String::new();
        let mut current_width = 0;

        for word in words {
            let word_width = display_width(word);

            if !current.is_empty() {
                let candidate_width = current_width + 1 + word_width;
                if candidate_width <= width {
                    current.push(' ');
                    current.push_str(word);
                    current_width = candidate_width;
                    continue;
                }

                lines.push(current);
                current = String::new();
            }

            if word_width <= width {
                current.push_str(word);
                current_width = word_width;
                continue;
            }

            let mut chunk = String::new();
            let mut chunk_width = 0;
            for character in word.chars() {
                let character_width = char_display_width(character);
                if chunk_width + character_width > width && !chunk.is_empty() {
                    lines.push(chunk);
                    chunk = String::new();
                    chunk_width = 0;
                }
                chunk.push(character);
                chunk_width += character_width;
            }

            current = chunk;
            current_width = chunk_width;
        }

        if !current.is_empty() {
            lines.push(current);
        }
    }

    if lines.is_empty() {
        vec!["-".to_string()]
    } else {
        lines
    }
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

fn char_display_width(character: char) -> usize {
    if character.is_ascii() || matches!(character, '·' | '•' | '…' | '█' | '░') {
        1
    } else {
        2
    }
}

fn pad_visible(text: &str, width: usize) -> String {
    let visible = display_width(text);
    if visible >= width {
        return text.to_string();
    }

    format!("{text}{}", " ".repeat(width - visible))
}

fn format_token_breakdown(value: &TokenBreakdown) -> String {
    if value.total_tokens == 0
        && value.input_tokens.is_none()
        && value.output_tokens.is_none()
        && value.cache_creation_input_tokens.is_none()
        && value.cache_read_input_tokens.is_none()
    {
        return "-".to_string();
    }

    let mut parts = vec![format!("{} total", format_number(value.total_tokens))];

    if let Some(input_tokens) = value.input_tokens {
        parts.push(format!("in {}", format_number(input_tokens)));
    }
    if let Some(output_tokens) = value.output_tokens {
        parts.push(format!("out {}", format_number(output_tokens)));
    }
    if let Some(cache_creation_input_tokens) = value.cache_creation_input_tokens {
        parts.push(format!(
            "cache+ {}",
            format_number(cache_creation_input_tokens)
        ));
    }
    if let Some(cache_read_input_tokens) = value.cache_read_input_tokens {
        parts.push(format!("cache~ {}", format_number(cache_read_input_tokens)));
    }

    parts.join(" · ")
}

fn format_optional_total(value: Option<u64>) -> String {
    value
        .map(|value| format!("{} tokens", format_number(value)))
        .unwrap_or_else(|| "-".to_string())
}

fn format_rate_limits(summary: &AiUsageSummary) -> String {
    let primary = format_rate_limit_window(&summary.codex.primary_rate_limit);
    let secondary = format_rate_limit_window(&summary.codex.secondary_rate_limit);

    if summary.codex.primary_rate_limit.used_percent.is_none()
        && summary.codex.primary_rate_limit.resets_at == "-"
        && summary.codex.secondary_rate_limit.used_percent.is_none()
        && summary.codex.secondary_rate_limit.resets_at == "-"
    {
        return summary.codex.hint.clone();
    }

    format!("{primary} / {secondary}")
}

fn format_rate_limit_window(window: &RateLimitWindow) -> String {
    let mut output = window.label.to_string();

    if let Some(used_percent) = window.used_percent {
        output.push(' ');
        output.push_str(&format!("{used_percent}%"));
    } else {
        output.push_str(" -");
    }

    if window.resets_at != "-" {
        output.push_str(" reset ");
        output.push_str(&window.resets_at);
    }

    output
}

fn format_number(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::new();

    for (index, character) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            output.push(',');
        }
        output.push(character);
    }

    output.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::model::{
        AiToolsSummary, AiUsageSummary, ClaudeUsageSummary, CodexUsageSummary, RateLimitWindow,
        ServiceEntry, ServiceManager, ServiceStatus, SystemSummary, TokenBreakdown,
        UsageAvailability, WelcomeSnapshot,
    };

    use super::{
        render_ai_tools_summary_with_width, render_ai_usage_summary_with_width, render_services,
        render_system_summary_with_width, render_welcome_with_width,
    };

    #[test]
    fn welcome_render_matches_wide_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_welcome_with_width(&snapshot, 100);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("welcome-wide.txt"))
        );
    }

    #[test]
    fn welcome_render_matches_narrow_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_welcome_with_width(&snapshot, 60);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("welcome-narrow.txt"))
        );
    }

    #[test]
    fn system_summary_render_matches_wide_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_system_summary_with_width(&snapshot.system, 100);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("system-summary-wide.txt"))
        );
    }

    #[test]
    fn system_summary_render_matches_narrow_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_system_summary_with_width(&snapshot.system, 60);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("system-summary-narrow.txt"))
        );
    }

    #[test]
    fn ai_tools_render_matches_wide_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_ai_tools_summary_with_width(&snapshot.ai_tools, 100);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-tools-wide.txt"))
        );
    }

    #[test]
    fn ai_tools_render_matches_narrow_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_ai_tools_summary_with_width(&snapshot.ai_tools, 60);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-tools-narrow.txt"))
        );
    }

    #[test]
    fn ai_usage_render_matches_wide_snapshot() {
        let summary = sample_ai_usage_summary();
        let output = render_ai_usage_summary_with_width(&summary, 100);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-usage-wide.txt"))
        );
    }

    #[test]
    fn ai_usage_render_matches_narrow_snapshot() {
        let summary = sample_ai_usage_summary();
        let output = render_ai_usage_summary_with_width(&summary, 60);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-usage-narrow.txt"))
        );
    }

    #[test]
    fn services_render_includes_entries() {
        let entries = vec![
            ServiceEntry {
                name: "cron".to_string(),
                manager: ServiceManager::Systemd,
                status: ServiceStatus::Running,
                detail: "active/running / enabled enabled".to_string(),
            },
            ServiceEntry {
                name: "gpu-manager".to_string(),
                manager: ServiceManager::Systemd,
                status: ServiceStatus::Stopped,
                detail: "inactive/dead / enabled enabled".to_string(),
            },
        ];

        let output = render_services(&entries);
        assert!(output.contains("Services"));
        assert!(output.contains("cron"));
        assert!(output.contains("gpu-manager"));
        assert!(output.contains("active/running / enabled enabled"));
    }

    fn sample_welcome_snapshot() -> WelcomeSnapshot {
        let fixture = fixture_map();

        WelcomeSnapshot {
            timestamp: fixture_value(&fixture, "timestamp"),
            user_label: fixture_value(&fixture, "user_label"),
            host_label: fixture_value(&fixture, "host_label"),
            current_dir: fixture_value(&fixture, "current_dir"),
            system: SystemSummary {
                os_label: fixture_value(&fixture, "system.os_label"),
                load_label: fixture_value(&fixture, "system.load_label"),
                host_label: fixture_value(&fixture, "system.host_label"),
                disk_label: fixture_value(&fixture, "system.disk_label"),
                cpu_label: fixture_value(&fixture, "system.cpu_label"),
                shell_label: fixture_value(&fixture, "system.shell_label"),
                memory_label: fixture_value(&fixture, "system.memory_label"),
                public_ip: crate::model::PublicIpSummary {
                    address: fixture_value(&fixture, "system.public_ip.address"),
                    country_label: fixture_value(&fixture, "system.public_ip.country_label"),
                    source: crate::model::PublicIpSource::Cache,
                },
                proxy_label: fixture_value(&fixture, "system.proxy_label"),
                uptime_label: fixture_value(&fixture, "system.uptime_label"),
            },
            ai_tools: AiToolsSummary {
                claude_model: fixture_value(&fixture, "ai_tools.claude_model"),
                claude_skills: fixture_value(&fixture, "ai_tools.claude_skills"),
                codex_model: fixture_value(&fixture, "ai_tools.codex_model"),
                codex_skills: fixture_value(&fixture, "ai_tools.codex_skills"),
            },
        }
    }

    fn sample_ai_usage_summary() -> AiUsageSummary {
        AiUsageSummary {
            claude: ClaudeUsageSummary {
                availability: UsageAvailability::Live,
                source_label: "local jsonl aggregate".to_string(),
                last_active_at: "2026-03-28 11:58 UTC".to_string(),
                last_24h: TokenBreakdown {
                    total_tokens: 225,
                    input_tokens: Some(150),
                    output_tokens: Some(60),
                    cache_creation_input_tokens: Some(10),
                    cache_read_input_tokens: Some(5),
                },
                last_7d: TokenBreakdown {
                    total_tokens: 265,
                    input_tokens: Some(180),
                    output_tokens: Some(70),
                    cache_creation_input_tokens: Some(10),
                    cache_read_input_tokens: Some(5),
                },
                lifetime: TokenBreakdown {
                    total_tokens: 295,
                    input_tokens: Some(200),
                    output_tokens: Some(80),
                    cache_creation_input_tokens: Some(10),
                    cache_read_input_tokens: Some(5),
                },
                hint: "Run /usage or /stats in Claude for plan limits".to_string(),
            },
            codex: CodexUsageSummary {
                availability: UsageAvailability::Live,
                plan_type: "pro".to_string(),
                last_active_at: "2026-03-28 11:59 UTC".to_string(),
                last_session_total_tokens: Some(200),
                last_24h_total_tokens: Some(90),
                last_7d_total_tokens: Some(140),
                primary_rate_limit: RateLimitWindow {
                    label: "Primary",
                    used_percent: Some(42),
                    resets_at: "2026-03-29 00:00 UTC".to_string(),
                },
                secondary_rate_limit: RateLimitWindow {
                    label: "Secondary",
                    used_percent: Some(12),
                    resets_at: "2026-03-30 00:00 UTC".to_string(),
                },
                hint: "Run /status in Codex for current usage".to_string(),
            },
            warnings: vec![
                "Claude skipped 1 malformed line".to_string(),
                "Codex auth plan type unavailable".to_string(),
            ],
        }
    }

    fn fixture_map() -> HashMap<String, String> {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/render-sample.txt"
        ))
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            trimmed
                .split_once('=')
                .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
    }

    fn fixture_value(fixture: &HashMap<String, String>, key: &str) -> String {
        fixture
            .get(key)
            .unwrap_or_else(|| panic!("missing fixture key: {key}"))
            .clone()
    }

    fn snapshot_text(name: &str) -> &'static str {
        match name {
            "welcome-wide.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/welcome-wide.txt"
            )),
            "welcome-narrow.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/welcome-narrow.txt"
            )),
            "system-summary-wide.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/system-summary-wide.txt"
            )),
            "system-summary-narrow.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/system-summary-narrow.txt"
            )),
            "ai-tools-wide.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/ai-tools-wide.txt"
            )),
            "ai-tools-narrow.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/ai-tools-narrow.txt"
            )),
            "ai-usage-wide.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/ai-usage-wide.txt"
            )),
            "ai-usage-narrow.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/ai-usage-narrow.txt"
            )),
            other => panic!("unknown snapshot: {other}"),
        }
    }

    fn normalize_snapshot(text: &str) -> &str {
        text.trim_end_matches('\n')
    }
}
