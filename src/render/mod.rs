use std::env;

use crate::model::{AiToolsSummary, SystemSummary, WelcomeSnapshot};

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
    output.push_str(&render_system_summary(&snapshot.system));
    output.push_str(&render_ai_tools_summary(&snapshot.ai_tools));

    output
}

pub fn render_system_summary(summary: &SystemSummary) -> String {
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
            "Proxy",
            summary.proxy_label.as_str(),
        ),
        (
            "Uptime",
            summary.uptime_label.as_str(),
            "Time",
            summary.time_label.as_str(),
        ),
    ];

    render_double_box_table("System Summary", &rows)
}

pub fn render_ai_tools_summary(summary: &AiToolsSummary) -> String {
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

    render_grouped_double_box_table("AI Tools", ("Claude", "Codex"), &rows)
}

fn render_double_box_table(title: &str, rows: &[(&str, &str, &str, &str)]) -> String {
    let layout = match resolve_double_table_layout(rows) {
        Some(layout) => layout,
        None => {
            let single_rows: Vec<(&str, &str)> = rows
                .iter()
                .flat_map(|(left_label, left_value, right_label, right_value)| {
                    [(*left_label, *left_value), (*right_label, *right_value)]
                })
                .collect();
            return render_box_table(title, &single_rows);
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
) -> String {
    let layout = match resolve_double_table_layout(rows) {
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
            return render_box_table_owned(title, &single_rows);
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

fn render_box_table(title: &str, rows: &[(&str, &str)]) -> String {
    let total_width = env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_WIDTH);
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

fn render_box_table_owned(title: &str, rows: &[(String, String)]) -> String {
    let borrowed_rows: Vec<(&str, &str)> = rows
        .iter()
        .map(|(label, value)| (label.as_str(), value.as_str()))
        .collect();
    render_box_table(title, &borrowed_rows)
}

fn resolve_double_table_layout(rows: &[(&str, &str, &str, &str)]) -> Option<DoubleTableLayout> {
    let total_width = env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_WIDTH);

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

#[cfg(test)]
mod tests {
    use crate::model::{AiToolsSummary, SystemSummary, WelcomeSnapshot};

    use super::{render_ai_tools_summary, render_system_summary, render_welcome};

    #[test]
    fn welcome_render_includes_core_fields() {
        let summary = SystemSummary {
            os_label: "macOS 15.0 (arm64)".to_string(),
            load_label: "1分 1.20 · 5分 1.10 · 15分 1.00".to_string(),
            host_label: "liupx-host".to_string(),
            disk_label: "█████░░░░░ 已用 120Gi / 总量 245Gi (49%)".to_string(),
            cpu_label: "Apple Silicon (arm64)".to_string(),
            shell_label: "zsh 5.9".to_string(),
            memory_label: "12.0 GiB used / 24.0 GiB total / 12.0 GiB avail".to_string(),
            proxy_label: "未启用".to_string(),
            uptime_label: "5 days, 3:01".to_string(),
            time_label: "2026-03-27 18:10".to_string(),
        };
        let snapshot = WelcomeSnapshot {
            timestamp: "2026-03-27 18:10".to_string(),
            user_label: "liupx".to_string(),
            host_label: "liupx-host".to_string(),
            current_dir: "~/git/pup-cli-start-rust".to_string(),
            system: summary,
            ai_tools: AiToolsSummary {
                claude_model: "claude-sonnet-4-6".to_string(),
                claude_skills: "(none)".to_string(),
                codex_model: "gpt-5.4".to_string(),
                codex_skills: "openai-docs".to_string(),
            },
        };

        let output = render_welcome(&snapshot);
        assert!(output.contains("Welcome back, liupx."));
        assert!(output.contains("liupx-host"));
        assert!(output.contains("System Summary"));
        assert!(output.contains("AI Tools"));
    }

    #[test]
    fn system_summary_render_includes_expected_labels() {
        let summary = SystemSummary {
            os_label: "macOS 15.0 (arm64)".to_string(),
            load_label: "1分 1.20 · 5分 1.10 · 15分 1.00".to_string(),
            host_label: "liupx-host".to_string(),
            disk_label: "█████░░░░░ 已用 120Gi / 总量 245Gi (49%)".to_string(),
            cpu_label: "Apple Silicon (arm64)".to_string(),
            shell_label: "zsh 5.9".to_string(),
            memory_label: "12.0 GiB used / 24.0 GiB total / 12.0 GiB avail".to_string(),
            proxy_label: "未启用".to_string(),
            uptime_label: "5 days, 3:01".to_string(),
            time_label: "2026-03-27 18:10".to_string(),
        };

        let output = render_system_summary(&summary);
        assert!(output.contains("OS"));
        assert!(output.contains("Disk"));
        assert!(output.contains("Time"));
        assert!(output.contains("liupx-host"));
    }

    #[test]
    fn ai_tools_render_includes_group_headers_and_values() {
        let summary = AiToolsSummary {
            claude_model: "claude-sonnet-4-6".to_string(),
            claude_skills: "refactor, review".to_string(),
            codex_model: "gpt-5.4".to_string(),
            codex_skills: "openai-docs".to_string(),
        };

        let output = render_ai_tools_summary(&summary);
        assert!(output.contains("AI Tools"));
        assert!(output.contains("Claude"));
        assert!(output.contains("Codex"));
        assert!(output.contains("refactor, review"));
        assert!(output.contains("openai-docs"));
    }
}
