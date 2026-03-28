use std::env;
use std::io::{self, IsTerminal};

use crate::model::{
    AiToolsSummary, AiUsageSummary, RateLimitWindow, ServiceEntry, SystemSummary, TokenBreakdown,
    WelcomeSnapshot,
};

const DEFAULT_WIDTH: usize = 100;
const MIN_LABEL_WIDTH: usize = 4;
const MIN_VALUE_WIDTH: usize = 16;
const ANSI_RESET: &str = "\u{1b}[0m";
const PROXY_ENABLED_STYLE: &str = "\u{1b}[30;48;5;151m";
const PROXY_DISABLED_STYLE: &str = "\u{1b}[30;48;5;223m";

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
        &snapshot.ai_usage,
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

pub fn render_ai_tools_summary(summary: &AiToolsSummary, usage: &AiUsageSummary) -> String {
    render_ai_tools_summary_with_width(summary, usage, resolve_total_width())
}

pub fn render_ai_skills_summary(summary: &AiToolsSummary) -> String {
    render_ai_skills_summary_with_width(summary, resolve_total_width())
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

fn render_ai_tools_summary_with_width(
    summary: &AiToolsSummary,
    usage: &AiUsageSummary,
    total_width: usize,
) -> String {
    render_ai_summary_table("AI Tools", Some(summary), usage, total_width)
}

fn render_ai_skills_summary_with_width(summary: &AiToolsSummary, total_width: usize) -> String {
    let rows = [(
        "Skills",
        summary.claude_skills.as_str(),
        "Skills",
        summary.codex_skills.as_str(),
    )];

    render_grouped_double_box_table("AI Skills", ("Claude", "Codex"), &rows, total_width)
}

fn render_ai_usage_summary_with_width(summary: &AiUsageSummary, total_width: usize) -> String {
    render_ai_summary_table("AI Usage", None, summary, total_width)
}

fn render_ai_summary_table(
    title: &str,
    tools: Option<&AiToolsSummary>,
    usage: &AiUsageSummary,
    total_width: usize,
) -> String {
    let claude_last_24h = format_token_breakdown(&usage.claude.last_24h);
    let claude_last_7d = format_token_breakdown(&usage.claude.last_7d);
    let claude_lifetime = format_token_breakdown(&usage.claude.lifetime);
    let codex_last_24h = format_optional_total(usage.codex.last_24h_total_tokens);
    let codex_last_7d = format_optional_total(usage.codex.last_7d_total_tokens);
    let codex_session = format_optional_total(usage.codex.last_session_total_tokens);
    let codex_limit_rows = format_codex_limit_rows(usage);
    let mut rows: Vec<(String, String, String, String)> = Vec::new();

    if let Some(tools) = tools {
        rows.push((
            "Model".to_string(),
            tools.claude_model.clone(),
            "Model".to_string(),
            tools.codex_model.clone(),
        ));
    }

    rows.extend([
        (
            "Source".to_string(),
            usage.claude.source_label.clone(),
            "Plan".to_string(),
            usage.codex.plan_type.clone(),
        ),
        (
            "Last Active".to_string(),
            usage.claude.last_active_at.clone(),
            "Last Active".to_string(),
            usage.codex.last_active_at.clone(),
        ),
        (
            "24h".to_string(),
            claude_last_24h,
            "24h".to_string(),
            codex_last_24h,
        ),
        (
            "7d".to_string(),
            claude_last_7d,
            "7d".to_string(),
            codex_last_7d,
        ),
        (
            "Lifetime".to_string(),
            claude_lifetime,
            "Session".to_string(),
            codex_session,
        ),
    ]);

    for (index, (codex_label, codex_value)) in codex_limit_rows.into_iter().enumerate() {
        let (claude_label, claude_value) = if index == 0 {
            ("Hint".to_string(), usage.claude.hint.clone())
        } else {
            (String::new(), String::new())
        };
        rows.push((claude_label, claude_value, codex_label, codex_value));
    }

    let mut output =
        render_grouped_double_box_table_owned(title, ("Claude", "Codex"), &rows, total_width);

    if !usage.warnings.is_empty() {
        let warning_rows: Vec<(String, String)> = usage
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
            let mut single_rows: Vec<(&str, &str)> = Vec::new();
            for (left_label, left_value, right_label, right_value) in rows {
                if !is_empty_cell(left_label, left_value) {
                    single_rows.push((*left_label, *left_value));
                }
                if !is_empty_cell(right_label, right_value) {
                    single_rows.push((*right_label, *right_value));
                }
            }
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
            let mut single_rows: Vec<(String, String)> = Vec::new();
            for (left_label, left_value, right_label, right_value) in rows {
                if !is_empty_cell(left_label, left_value) {
                    single_rows.push((
                        format_grouped_label(group_headers.0, left_label),
                        (*left_value).to_string(),
                    ));
                }
                if !is_empty_cell(right_label, right_value) {
                    single_rows.push((
                        format_grouped_label(group_headers.1, right_label),
                        (*right_value).to_string(),
                    ));
                }
            }
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

fn render_grouped_double_box_table_owned(
    title: &str,
    group_headers: (&str, &str),
    rows: &[(String, String, String, String)],
    total_width: usize,
) -> String {
    let borrowed_rows: Vec<(&str, &str, &str, &str)> = rows
        .iter()
        .map(|(left_label, left_value, right_label, right_value)| {
            (
                left_label.as_str(),
                left_value.as_str(),
                right_label.as_str(),
                right_value.as_str(),
            )
        })
        .collect();

    render_grouped_double_box_table(title, group_headers, &borrowed_rows, total_width)
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
            output.push_str(&render_value_cell(label, value_text, value_width));
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
            output.push_str(&render_value_cell(
                left_label,
                left_value_text,
                layout.left_value_width,
            ));
            output.push_str(" │ ");
            output.push_str(&pad_visible(right_label_text, layout.label_width));
            output.push_str(" │ ");
            output.push_str(&render_value_cell(
                right_label,
                right_value_text,
                layout.right_value_width,
            ));
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

    if text.trim().is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();

    for raw_line in text.lines() {
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
        vec![String::new()]
    } else {
        lines
    }
}

fn display_width(text: &str) -> usize {
    text.chars().map(char_display_width).sum()
}

fn char_display_width(character: char) -> usize {
    if character.is_ascii()
        || matches!(character, '·' | '•' | '…' | '█' | '░')
        || is_regional_indicator(character)
    {
        1
    } else {
        2
    }
}

fn is_regional_indicator(character: char) -> bool {
    matches!(u32::from(character), 0x1F1E6..=0x1F1FF)
}

fn pad_visible(text: &str, width: usize) -> String {
    let visible = display_width(text);
    if visible >= width {
        return text.to_string();
    }

    format!("{text}{}", " ".repeat(width - visible))
}

fn render_value_cell(label: &str, text: &str, width: usize) -> String {
    let padded = pad_visible(text, width);

    if label != "Proxy" || !can_use_ansi_color() {
        return padded;
    }

    let style = if text.trim_start().starts_with("已启用") {
        Some(PROXY_ENABLED_STYLE)
    } else if text.trim_start().starts_with("未启用") {
        Some(PROXY_DISABLED_STYLE)
    } else {
        None
    };

    match style {
        Some(style) => format!("{style}{padded}{ANSI_RESET}"),
        None => padded,
    }
}

fn can_use_ansi_color() -> bool {
    io::stdout().is_terminal()
        && env::var("TERM")
            .map(|value| value != "dumb")
            .unwrap_or(true)
        && env::var_os("NO_COLOR").is_none()
}

fn format_grouped_label(group_header: &str, label: &str) -> String {
    if label.trim().is_empty() {
        group_header.to_string()
    } else {
        format!("{group_header} {label}")
    }
}

fn is_empty_cell(label: &str, value: &str) -> bool {
    label.trim().is_empty() && value.trim().is_empty()
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

fn format_codex_limit_rows(summary: &AiUsageSummary) -> Vec<(String, String)> {
    let mut rows = Vec::new();

    if let Some(context_window) = format_context_window(summary) {
        rows.push(("Context".to_string(), context_window));
    }

    if let Some(primary) = format_rate_limit_value(summary, &summary.codex.primary_rate_limit) {
        rows.push((
            format_limit_label(&summary.codex.primary_rate_limit),
            primary,
        ));
    }

    if let Some(secondary) = format_rate_limit_value(summary, &summary.codex.secondary_rate_limit) {
        rows.push((
            format_limit_label(&summary.codex.secondary_rate_limit),
            secondary,
        ));
    }

    if rows.is_empty() {
        rows.push(("Hint".to_string(), summary.codex.hint.clone()));
    }

    rows
}

fn format_context_window(summary: &AiUsageSummary) -> Option<String> {
    let session_total_tokens = summary.codex.last_session_total_tokens?;
    let total_tokens = summary.codex.model_context_window?;
    if total_tokens == 0 {
        return None;
    }

    Some(format!(
        "{} window (session total {})",
        format_compact_number(total_tokens),
        format_compact_number(session_total_tokens)
    ))
}

fn format_rate_limit_value(summary: &AiUsageSummary, window: &RateLimitWindow) -> Option<String> {
    let used_percent = window.used_percent?;
    let remaining_percent = 100_u8.saturating_sub(used_percent);
    let reset_label = format_rate_limit_reset(&window.resets_at, &summary.codex.last_active_at);

    Some(format!(
        "[{}] {}% left ({})",
        format_remaining_bar(remaining_percent, 10),
        remaining_percent,
        reset_label
    ))
}

fn format_limit_label(window: &RateLimitWindow) -> String {
    match window.window_minutes {
        Some(300) => "5h limit".to_string(),
        Some(10_080) => "Weekly limit".to_string(),
        Some(minutes) if minutes % 1_440 == 0 => format!("{}d limit", minutes / 1_440),
        Some(minutes) if minutes % 60 == 0 => format!("{}h limit", minutes / 60),
        Some(minutes) => format!("{}m limit", minutes),
        None => format!("{} limit", window.label),
    }
}

fn format_remaining_bar(remaining_percent: u8, slots: usize) -> String {
    let filled = (((remaining_percent as usize) * slots) + 50) / 100;
    let filled = filled.min(slots);
    format!("{}{}", "█".repeat(filled), "░".repeat(slots - filled))
}

fn format_rate_limit_reset(resets_at: &str, last_active_at: &str) -> String {
    let Some((reset_date, reset_time)) = parse_rendered_utc_timestamp(resets_at) else {
        return if resets_at == "-" {
            "resets unknown".to_string()
        } else {
            format!("resets {resets_at}")
        };
    };
    let active_date = parse_rendered_utc_timestamp(last_active_at).map(|(date, _)| date);

    if active_date.as_deref() == Some(reset_date.as_str()) {
        format!("resets {reset_time} UTC")
    } else {
        format!(
            "resets {} UTC on {}",
            reset_time,
            format_short_date(&reset_date)
        )
    }
}

fn parse_rendered_utc_timestamp(value: &str) -> Option<(String, String)> {
    let trimmed = value.trim();
    let stripped = trimmed.strip_suffix(" UTC")?;
    let (date, time) = stripped.split_once(' ')?;
    Some((date.to_string(), time.to_string()))
}

fn format_short_date(date: &str) -> String {
    let mut parts = date.split('-');
    let Some(_year) = parts.next() else {
        return date.to_string();
    };
    let Some(month) = parts.next() else {
        return date.to_string();
    };
    let Some(day) = parts.next() else {
        return date.to_string();
    };
    let month = match month {
        "01" => "Jan",
        "02" => "Feb",
        "03" => "Mar",
        "04" => "Apr",
        "05" => "May",
        "06" => "Jun",
        "07" => "Jul",
        "08" => "Aug",
        "09" => "Sep",
        "10" => "Oct",
        "11" => "Nov",
        "12" => "Dec",
        _ => return date.to_string(),
    };
    let day = day.trim_start_matches('0');
    format!("{day} {month}")
}

fn format_compact_number(value: u64) -> String {
    const UNITS: [(&str, f64); 4] = [
        ("B", 1_000_000_000.0),
        ("M", 1_000_000.0),
        ("K", 1_000.0),
        ("", 1.0),
    ];

    for (suffix, divisor) in UNITS {
        if value as f64 >= divisor {
            if suffix.is_empty() {
                return format_number(value);
            }

            let number = value as f64 / divisor;
            let formatted = if number >= 100.0 {
                format!("{number:.0}")
            } else if number >= 10.0 {
                format!("{number:.1}")
            } else {
                format!("{number:.2}")
            };
            return format!(
                "{}{}",
                formatted.trim_end_matches('0').trim_end_matches('.'),
                suffix
            );
        }
    }

    format_number(value)
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
        render_ai_skills_summary_with_width, render_ai_tools_summary_with_width,
        render_ai_usage_summary_with_width, render_services, render_system_summary_with_width,
        render_welcome_with_width,
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
        let output =
            render_ai_tools_summary_with_width(&snapshot.ai_tools, &snapshot.ai_usage, 100);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-tools-wide.txt"))
        );
    }

    #[test]
    fn ai_tools_render_matches_narrow_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_ai_tools_summary_with_width(&snapshot.ai_tools, &snapshot.ai_usage, 60);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-tools-narrow.txt"))
        );
    }

    #[test]
    fn ai_skills_render_matches_wide_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_ai_skills_summary_with_width(&snapshot.ai_tools, 100);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-skills-wide.txt"))
        );
    }

    #[test]
    fn ai_skills_render_matches_narrow_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_ai_skills_summary_with_width(&snapshot.ai_tools, 60);

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("ai-skills-narrow.txt"))
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
            ai_usage: {
                let mut summary = sample_ai_usage_summary();
                summary.warnings.clear();
                summary
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
                last_session_total_tokens: Some(62_300),
                model_context_window: Some(258_400),
                last_24h_total_tokens: Some(124_000),
                last_7d_total_tokens: Some(450_000),
                primary_rate_limit: RateLimitWindow {
                    label: "Primary",
                    used_percent: Some(42),
                    window_minutes: Some(300),
                    resets_at: "2026-03-29 00:00 UTC".to_string(),
                },
                secondary_rate_limit: RateLimitWindow {
                    label: "Secondary",
                    used_percent: Some(12),
                    window_minutes: Some(10_080),
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
            "ai-skills-wide.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/ai-skills-wide.txt"
            )),
            "ai-skills-narrow.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/ai-skills-narrow.txt"
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
