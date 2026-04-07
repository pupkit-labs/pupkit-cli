use std::env;
use std::io::{self, IsTerminal};
use std::process::Command;

use crate::model::{
    AiToolsSummary, AiUsageSummary, CopilotQuotaEntry, CopilotUsageSummary, RateLimitWindow,
    TokenBreakdown, WelcomeSnapshot,
};

const DEFAULT_WIDTH: usize = 100;
const MIN_LABEL_WIDTH: usize = 4;
const MIN_VALUE_WIDTH: usize = 16;
const ANSI_RESET: &str = "\u{1b}[0m";
const CYAN_STYLE: &str = "\u{1b}[38;2;32;201;255m";
const TITLE_GRADIENT_STOPS: &[(u8, u8, u8)] = &[(180, 92, 255), (84, 119, 255), (32, 201, 255)];
const PROXY_ENABLED_STYLE: &str = "\u{1b}[30;48;2;92;255;128m";
const PROXY_DISABLED_STYLE: &str = "\u{1b}[30;48;2;255;210;150m";
pub const IP_LOADING_TEXT: &str = "Loading...";
pub const COPILOT_LOADING_TEXT: &str = "Loading Copilot...";

pub mod ansi {
    pub const HIDE_CURSOR: &str = "\x1b[?25l";
    pub const SHOW_CURSOR: &str = "\x1b[?25h";
    pub const CLEAR_LINE: &str = "\x1b[2K";
    pub const CLEAR_UNTIL_END: &str = "\x1b[0J";
    pub const LOADING_FRAME_INTERVAL_MILLIS: u64 = 120;
    const LOADING_SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    pub fn move_up(lines: usize) -> String {
        if lines == 0 {
            String::new()
        } else {
            format!("\x1b[{lines}A")
        }
    }

    pub fn move_to_column(column: usize) -> String {
        format!("\x1b[{}G", column.max(1))
    }

    pub fn line_count(text: &str) -> usize {
        text.lines().count()
    }

    pub fn loading_spinner_frame(frame_index: usize) -> &'static str {
        LOADING_SPINNER_FRAMES[frame_index % LOADING_SPINNER_FRAMES.len()]
    }

    pub fn animated_loading_label(label: &str, frame_index: usize) -> String {
        format!("{} {}", loading_spinner_frame(frame_index), label)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalTimeContext {
    offset_minutes: i32,
    offset_label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SimpleDateTime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
}

pub fn render_welcome_slim(snapshot: &WelcomeSnapshot) -> String {
    render_welcome_slim_with_width(snapshot, resolve_total_width())
}

pub fn render_welcome_loading(snapshot: &WelcomeSnapshot) -> String {
    render_welcome_loading_frame(snapshot, 0)
}

pub fn render_welcome_loading_frame(snapshot: &WelcomeSnapshot, frame_index: usize) -> String {
    render_welcome_loading_with_width(snapshot, resolve_total_width(), frame_index)
}

pub fn render_refresh(previous_output: &str, next_output: &str) -> String {
    if previous_output.is_empty() {
        return next_output.to_string();
    }

    format!(
        "{}{}{}{}{}",
        ansi::move_up(ansi::line_count(previous_output)),
        ansi::move_to_column(1),
        ansi::CLEAR_LINE,
        ansi::CLEAR_UNTIL_END,
        next_output,
    )
}

fn render_welcome_loading_with_width(
    snapshot: &WelcomeSnapshot,
    total_width: usize,
    frame_index: usize,
) -> String {
    let local_time = resolve_local_time_context();
    render_welcome_loading_with_width_and_context(snapshot, total_width, &local_time, frame_index)
}

fn render_welcome_loading_with_width_and_context(
    snapshot: &WelcomeSnapshot,
    total_width: usize,
    local_time: &LocalTimeContext,
    frame_index: usize,
) -> String {
    render_welcome_with_width_and_context(snapshot, total_width, local_time, Some(frame_index))
}

fn render_welcome_slim_with_width(snapshot: &WelcomeSnapshot, total_width: usize) -> String {
    let local_time = resolve_local_time_context();
    render_welcome_slim_with_width_and_context(snapshot, total_width, &local_time)
}

fn render_welcome_slim_with_width_and_context(
    snapshot: &WelcomeSnapshot,
    total_width: usize,
    local_time: &LocalTimeContext,
) -> String {
    render_welcome_with_width_and_context(snapshot, total_width, local_time, None)
}

fn render_welcome_with_width_and_context(
    snapshot: &WelcomeSnapshot,
    total_width: usize,
    local_time: &LocalTimeContext,
    loading_frame: Option<usize>,
) -> String {
    let mut output = String::new();

    output.push('\n');
    output.push_str(&render_title_art());
    output.push('\n');
    output.push_str(&format!("Welcome back, {}.\n", snapshot.user_label));
    output.push_str(&format!(
        "🕐 {}   👤 {}@{}\n\n",
        snapshot.timestamp, snapshot.user_label, snapshot.system.host_label
    ));

    let ai_section = render_ai_slim_section(
        &snapshot.ai_tools,
        &snapshot.ai_usage,
        &snapshot.copilot,
        total_width,
        local_time,
        loading_frame,
    );
    let separator = ai_section.lines().next().unwrap_or("").to_string();
    let public_ip_label = if snapshot.system.public_ip.is_loading {
        loading_frame
            .map(|frame_index| ansi::animated_loading_label(IP_LOADING_TEXT, frame_index))
            .unwrap_or_else(|| IP_LOADING_TEXT.to_string())
    } else {
        snapshot.system.public_ip.display_label()
    };

    output.push_str(&separator);
    output.push('\n');
    output.push_str(&render_network_kv(
        &public_ip_label,
        &snapshot.system.proxy_label,
    ));
    output.push_str(&ai_section);

    output
}

fn render_network_kv(ip_label: &str, proxy_label: &str) -> String {
    let ip_key_width = 2;
    let proxy_key_width = 5;
    let proxy_highlighted = if can_use_ansi_color() {
        let style = if proxy_label.trim_start().starts_with("已启用") {
            Some(PROXY_ENABLED_STYLE)
        } else if proxy_label.trim_start().starts_with("未启用") {
            Some(PROXY_DISABLED_STYLE)
        } else {
            None
        };

        match style {
            Some(style) => format!("{style} {} {ANSI_RESET}", proxy_label.trim()),
            None => proxy_label.to_string(),
        }
    } else {
        proxy_label.to_string()
    };

    format!(
        "  {:<ip_key_width$}  {}    {:<proxy_key_width$}  {}\n",
        "IP", ip_label, "Proxy", proxy_highlighted,
    )
}

fn render_ai_slim_section(
    tools: &AiToolsSummary,
    usage: &AiUsageSummary,
    copilot: &CopilotUsageSummary,
    total_width: usize,
    local_time: &LocalTimeContext,
    loading_frame: Option<usize>,
) -> String {
    let claude_24h = format_token_breakdown_compact(&usage.claude.last_24h);
    let mut claude_items: Vec<(String, String)> = vec![
        ("Model".to_string(), tools.claude_model.clone()),
        ("24h".to_string(), claude_24h),
    ];
    claude_items.push((
        "7d".to_string(),
        format_token_breakdown_compact(&usage.claude.last_7d),
    ));

    let codex_primary_label = format_limit_label(&usage.codex.primary_rate_limit);
    let codex_primary_value =
        format_rate_limit_value_slim(usage, &usage.codex.primary_rate_limit, local_time)
            .unwrap_or_else(|| "-".to_string());
    let codex_secondary_label = format_limit_label(&usage.codex.secondary_rate_limit);
    let codex_secondary_value =
        format_rate_limit_value_slim(usage, &usage.codex.secondary_rate_limit, local_time)
            .unwrap_or_else(|| "-".to_string());

    let copilot_req_str = copilot.total_requests.map(|value| {
        let per_day = copilot
            .last_24h_requests
            .map(|day| format!(" · 24h {}", format_number(day)))
            .unwrap_or_default();
        format!("{}{}", format_number(value), per_day)
    });
    let copilot_sessions_str = copilot.total_sessions.map(format_number);

    let mut copilot_items: Vec<(String, String)> = Vec::new();
    if copilot.is_loading {
        let loading_label = loading_frame
            .map(|frame_index| ansi::animated_loading_label(COPILOT_LOADING_TEXT, frame_index))
            .unwrap_or_else(|| COPILOT_LOADING_TEXT.to_string());
        copilot_items.push(("Plan".to_string(), loading_label));
    }
    if let Some(ref quota) = copilot.quota {
        copilot_items.push(("Plan".to_string(), quota.plan.clone()));
        copilot_items.push(("Premium".to_string(), format_quota_entry(&quota.premium)));
        copilot_items.push(("Chat".to_string(), format_quota_entry(&quota.chat)));
        copilot_items.push((
            "Complete".to_string(),
            format_quota_entry(&quota.completions),
        ));
        copilot_items.push(("Reset".to_string(), quota.reset_date.clone()));
    } else {
        copilot_items.push(("Model".to_string(), copilot.model.clone()));
    }
    copilot_items.push((
        "Total Req".to_string(),
        copilot_req_str.unwrap_or_else(|| "-".to_string()),
    ));
    copilot_items.push((
        "Sessions".to_string(),
        copilot_sessions_str.unwrap_or_else(|| "-".to_string()),
    ));
    if !copilot.is_loading && copilot.quota.is_none() && copilot.total_requests.is_none() {
        copilot_items.push(("Hint".to_string(), copilot.hint.clone()));
    }

    let groups: Vec<(&str, Vec<(String, String)>)> = vec![
        ("Claude", claude_items),
        (
            "Codex",
            vec![
                ("Model".to_string(), tools.codex_model.clone()),
                (codex_primary_label, codex_primary_value),
                (codex_secondary_label, codex_secondary_value),
            ],
        ),
        ("Copilot", copilot_items),
    ];

    let raw_table = render_grouped_ai_table("AI Quick Look", &groups, total_width);
    let table_width = raw_table
        .lines()
        .find(|line| line.starts_with('┌'))
        .map(display_width)
        .unwrap_or(total_width);
    let raw_separator: String = (0..table_width)
        .map(|index| if index % 2 == 0 { '~' } else { '+' })
        .collect();
    let separator = style_separator_line(&raw_separator);
    let table = style_table_frame(&raw_table);

    format!("{separator}\n{table}")
}

fn render_grouped_ai_table(
    title: &str,
    groups: &[(&str, Vec<(String, String)>)],
    total_width: usize,
) -> String {
    let label_width = groups
        .iter()
        .flat_map(|(group, items)| {
            let item_count = items.len();
            let indent_len = display_width(group);
            items
                .iter()
                .enumerate()
                .map(move |(index, (sub_label, _))| {
                    if item_count == 1 || index == 0 {
                        display_width(group) + 3 + display_width(sub_label)
                    } else {
                        indent_len + 3 + display_width(sub_label)
                    }
                })
        })
        .max()
        .unwrap_or(MIN_LABEL_WIDTH)
        .max(MIN_LABEL_WIDTH);
    let max_content_width = groups
        .iter()
        .flat_map(|(_, items)| items.iter())
        .flat_map(|(_, value)| value.split('\n'))
        .map(display_width)
        .max()
        .unwrap_or(MIN_VALUE_WIDTH);
    let available_value_width = total_width
        .saturating_sub(label_width + 7)
        .max(MIN_VALUE_WIDTH);
    let value_width = max_content_width
        .max(MIN_VALUE_WIDTH)
        .min(available_value_width);
    let label_col_width = label_width + 2;
    let value_col_width = value_width + 2;

    let mut output = String::new();
    output.push_str(title);
    output.push('\n');
    output.push_str(&render_border(
        "┌",
        "┬",
        "┐",
        &[label_col_width, value_col_width],
    ));

    for (group_index, (group, items)) in groups.iter().enumerate() {
        let item_count = items.len();
        let group_count = groups.len();
        let indent = " ".repeat(display_width(group));

        for (item_index, (sub_label, value)) in items.iter().enumerate() {
            let is_last_item = item_index + 1 == item_count;
            let tree_label = if item_count == 1 {
                format!("{group} ─ {sub_label}")
            } else if item_index == 0 {
                format!("{group} ┬ {sub_label}")
            } else if is_last_item {
                format!("{indent} └ {sub_label}")
            } else {
                format!("{indent} ├ {sub_label}")
            };
            let continuation_label = if !is_last_item {
                format!("{indent} │")
            } else {
                String::new()
            };

            let value_lines = wrap_text(value, value_width);
            for (line_index, value_line) in value_lines.iter().enumerate() {
                let label_text = if line_index == 0 {
                    tree_label.as_str()
                } else {
                    continuation_label.as_str()
                };

                output.push_str("│ ");
                output.push_str(&pad_visible(label_text, label_width));
                output.push_str(" │ ");
                output.push_str(&pad_visible(value_line, value_width));
                output.push_str(" │\n");
            }

            let is_last_group = group_index + 1 == group_count;
            if is_last_item && is_last_group {
                output.push_str(&render_border(
                    "└",
                    "┴",
                    "┘",
                    &[label_col_width, value_col_width],
                ));
            } else if is_last_item {
                output.push_str(&render_border(
                    "├",
                    "┼",
                    "┤",
                    &[label_col_width, value_col_width],
                ));
            } else {
                let remaining = label_width.saturating_sub(indent.len() + 2);
                output.push('│');
                output.push(' ');
                output.push_str(&indent);
                output.push(' ');
                output.push('│');
                output.push_str(&" ".repeat(remaining));
                output.push(' ');
                output.push('├');
                output.push_str(&"─".repeat(value_col_width));
                output.push_str("┤\n");
            }
        }
    }

    output.push('\n');
    output
}

fn resolve_total_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_WIDTH)
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
        || is_braille_pattern(character)
        || is_regional_indicator(character)
        || matches!(u32::from(character), 0x2500..=0x257F)
    {
        1
    } else {
        2
    }
}

fn is_regional_indicator(character: char) -> bool {
    matches!(u32::from(character), 0x1F1E6..=0x1F1FF)
}

fn is_braille_pattern(character: char) -> bool {
    matches!(u32::from(character), 0x2800..=0x28FF)
}

fn is_box_drawing(character: char) -> bool {
    matches!(u32::from(character), 0x2500..=0x257F)
}

fn pad_visible(text: &str, width: usize) -> String {
    let visible = display_width(text);
    if visible >= width {
        return text.to_string();
    }

    format!("{text}{}", " ".repeat(width - visible))
}

fn can_use_ansi_color() -> bool {
    io::stdout().is_terminal()
        && env::var("TERM")
            .map(|value| value != "dumb")
            .unwrap_or(true)
        && env::var_os("NO_COLOR").is_none()
}

fn style_separator_line(text: &str) -> String {
    if !can_use_ansi_color() {
        return text.to_string();
    }

    format!("{CYAN_STYLE}{text}{ANSI_RESET}")
}

fn style_table_frame(text: &str) -> String {
    if !can_use_ansi_color() {
        return text.to_string();
    }

    let mut output = String::with_capacity(text.len());
    for character in text.chars() {
        if is_box_drawing(character) {
            output.push_str(CYAN_STYLE);
            output.push(character);
            output.push_str(ANSI_RESET);
        } else {
            output.push(character);
        }
    }

    output
}

fn render_title_art() -> String {
    const TITLE_LINES: [&str; 5] = [
        " ____   _   _  ____   _  __  ___  _____ ",
        "|  _ \\ | | | ||  _ \\ | |/ / |_ _||_   _|",
        "| |_) || | | || |_) || ' /   | |   | |  ",
        "|  __/ | |_| ||  __/ | . \\   | |   | |  ",
        "|_|     \\___/ |_|    |_|\\_\\ |___|  |_|  ",
    ];

    if !can_use_ansi_color() {
        return format!("{}\n", TITLE_LINES.join("\n"));
    }

    let total_visible_chars = TITLE_LINES
        .iter()
        .flat_map(|line| line.chars())
        .filter(|character| !character.is_whitespace())
        .count();
    let mut output = String::new();
    let mut visible_index = 0usize;

    for line in TITLE_LINES {
        for character in line.chars() {
            if character.is_whitespace() {
                output.push(character);
                continue;
            }

            let (red, green, blue) =
                gradient_color(visible_index, total_visible_chars, TITLE_GRADIENT_STOPS);
            output.push_str(&format!(
                "\u{1b}[38;2;{red};{green};{blue}m{character}{ANSI_RESET}"
            ));
            visible_index += 1;
        }
        output.push('\n');
    }

    output
}

fn gradient_color(index: usize, total: usize, stops: &[(u8, u8, u8)]) -> (u8, u8, u8) {
    if stops.is_empty() {
        return (255, 255, 255);
    }
    if stops.len() == 1 || total <= 1 {
        return stops[0];
    }

    let segments = stops.len() - 1;
    let scaled = index.saturating_mul(segments);
    let start_index = (scaled / (total - 1)).min(segments - 1);
    let end_index = (start_index + 1).min(stops.len() - 1);
    let segment_start = start_index * (total - 1) / segments;
    let segment_end = ((start_index + 1) * (total - 1)) / segments;
    let span = segment_end.saturating_sub(segment_start).max(1);
    let offset = index.saturating_sub(segment_start).min(span);
    let t = offset as f32 / span as f32;

    interpolate_rgb(stops[start_index], stops[end_index], t)
}

fn interpolate_rgb(start: (u8, u8, u8), end: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let lerp = |a: u8, b: u8| -> u8 {
        let value = a as f32 + (b as f32 - a as f32) * t;
        value.round().clamp(0.0, 255.0) as u8
    };

    (
        lerp(start.0, end.0),
        lerp(start.1, end.1),
        lerp(start.2, end.2),
    )
}

fn format_token_breakdown_compact(value: &TokenBreakdown) -> String {
    if value.total_tokens == 0
        && value.input_tokens.is_none()
        && value.output_tokens.is_none()
        && value.cache_creation_input_tokens.is_none()
        && value.cache_read_input_tokens.is_none()
    {
        return "-".to_string();
    }

    let mut parts = vec![format_compact_number(value.total_tokens)];
    if let Some(input_tokens) = value.input_tokens {
        parts.push(format!("in {}", format_compact_number(input_tokens)));
    }
    if let Some(output_tokens) = value.output_tokens {
        parts.push(format!("out {}", format_compact_number(output_tokens)));
    }
    if let Some(cache_creation_tokens) = value.cache_creation_input_tokens {
        if cache_creation_tokens > 0 {
            parts.push(format!(
                "cache+ {}",
                format_compact_number(cache_creation_tokens)
            ));
        }
    }
    if let Some(cache_read_tokens) = value.cache_read_input_tokens {
        if cache_read_tokens > 0 {
            parts.push(format!(
                "cache~ {}",
                format_compact_number(cache_read_tokens)
            ));
        }
    }

    parts.join(" · ")
}

fn format_rate_limit_value_slim(
    summary: &AiUsageSummary,
    window: &RateLimitWindow,
    local_time: &LocalTimeContext,
) -> Option<String> {
    let used_percent = window.used_percent?;
    let remaining_percent = 100_u8.saturating_sub(used_percent);
    let reset_label = format_rate_limit_reset(window, &summary.codex.last_active_at, local_time);

    Some(format!(
        "[{}] {}% left\n{}",
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

fn format_quota_entry(entry: &CopilotQuotaEntry) -> String {
    if entry.unlimited {
        return "unlimited".to_string();
    }

    let whole = entry.percent_remaining_x10 / 10;
    let frac = entry.percent_remaining_x10 % 10;
    format!(
        "{}/{} ({whole}.{frac}%)",
        format_number(entry.remaining),
        format_number(entry.entitlement),
    )
}

fn format_remaining_bar(remaining_percent: u8, slots: usize) -> String {
    let filled = (((remaining_percent as usize) * slots) + 50) / 100;
    let filled = filled.min(slots);
    format!("{}{}", "█".repeat(filled), "░".repeat(slots - filled))
}

fn format_rate_limit_reset(
    window: &RateLimitWindow,
    last_active_at: &str,
    local_time: &LocalTimeContext,
) -> String {
    let Some(reset_local) = localize_rate_limit_reset(window, local_time) else {
        return if window.resets_at == "-" {
            "resets unknown".to_string()
        } else {
            format!("resets {}", window.resets_at)
        };
    };
    let active_local = parse_rendered_utc_timestamp(last_active_at)
        .and_then(utc_datetime_to_epoch_secs)
        .map(|epoch_secs| local_datetime_from_epoch_secs(epoch_secs, local_time.offset_minutes));

    if active_local.map(|active| (active.year, active.month, active.day))
        == Some((reset_local.year, reset_local.month, reset_local.day))
    {
        format!(
            "resets {:02}:{:02} {}",
            reset_local.hour, reset_local.minute, local_time.offset_label
        )
    } else {
        format!(
            "resets {:02}:{:02} {} on {}",
            reset_local.hour,
            reset_local.minute,
            local_time.offset_label,
            format_short_date_parts(reset_local.month, reset_local.day)
        )
    }
}

fn localize_rate_limit_reset(
    window: &RateLimitWindow,
    local_time: &LocalTimeContext,
) -> Option<SimpleDateTime> {
    if let Some(epoch_secs) = window.resets_at_epoch_secs {
        return Some(local_datetime_from_epoch_secs(
            epoch_secs as i64,
            local_time.offset_minutes,
        ));
    }

    parse_rendered_utc_timestamp(&window.resets_at)
        .and_then(utc_datetime_to_epoch_secs)
        .map(|epoch_secs| local_datetime_from_epoch_secs(epoch_secs, local_time.offset_minutes))
}

fn parse_rendered_utc_timestamp(value: &str) -> Option<SimpleDateTime> {
    let stripped = value.trim().strip_suffix(" UTC")?;
    let (date, time) = stripped.split_once(' ')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    let (hour, minute) = time.split_once(':')?;

    Some(SimpleDateTime {
        year,
        month,
        day,
        hour: hour.parse::<u32>().ok()?,
        minute: minute.parse::<u32>().ok()?,
    })
}

fn format_short_date_parts(month: u32, day: u32) -> String {
    let month = match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => return format!("{day}"),
    };

    format!("{day} {month}")
}

fn resolve_local_time_context() -> LocalTimeContext {
    let offset_minutes = detect_local_utc_offset_minutes().unwrap_or(0);

    LocalTimeContext {
        offset_minutes,
        offset_label: format_utc_offset_label(offset_minutes),
    }
}

fn detect_local_utc_offset_minutes() -> Option<i32> {
    run_command("date", &["+%z"]).and_then(|value| parse_utc_offset_minutes(&value))
}

fn parse_utc_offset_minutes(value: &str) -> Option<i32> {
    let trimmed = value.trim();
    let sign = match trimmed.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits = trimmed.trim_start_matches(['+', '-']);
    if digits.len() != 4 {
        return None;
    }

    let hours = digits[..2].parse::<i32>().ok()?;
    let minutes = digits[2..].parse::<i32>().ok()?;

    Some(sign * (hours * 60 + minutes))
}

fn format_utc_offset_label(offset_minutes: i32) -> String {
    if offset_minutes == 0 {
        return "UTC".to_string();
    }

    let sign = if offset_minutes >= 0 { '+' } else { '-' };
    let absolute = offset_minutes.abs();
    let hours = absolute / 60;
    let minutes = absolute % 60;

    format!("UTC{sign}{hours:02}:{minutes:02}")
}

fn utc_datetime_to_epoch_secs(datetime: SimpleDateTime) -> Option<i64> {
    if datetime.month == 0
        || datetime.month > 12
        || datetime.day == 0
        || datetime.day > 31
        || datetime.hour > 23
        || datetime.minute > 59
    {
        return None;
    }

    let days = days_from_civil(datetime.year, datetime.month, datetime.day);
    Some(days * 86_400 + i64::from(datetime.hour) * 3_600 + i64::from(datetime.minute) * 60)
}

fn local_datetime_from_epoch_secs(epoch_secs: i64, offset_minutes: i32) -> SimpleDateTime {
    let localized_secs = epoch_secs + i64::from(offset_minutes) * 60;
    let days = localized_secs.div_euclid(86_400);
    let seconds_of_day = localized_secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);

    SimpleDateTime {
        year,
        month,
        day,
        hour: (seconds_of_day / 3_600) as u32,
        minute: ((seconds_of_day % 3_600) / 60) as u32,
    }
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month as i32 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day as i32 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    i64::from(era) * 146_097 + i64::from(day_of_era) - 719_468
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let shifted_days = days_since_epoch + 719_468;
    let era = if shifted_days >= 0 {
        shifted_days
    } else {
        shifted_days - 146_096
    } / 146_097;
    let day_of_era = shifted_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i32 + era as i32 * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_piece = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_piece + 2) / 5 + 1;
    let month = month_piece + if month_piece < 10 { 3 } else { -9 };

    (
        year + if month <= 2 { 1 } else { 0 },
        month as u32,
        day as u32,
    )
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

    use super::*;
    use super::{
        render_welcome_loading_with_width_and_context, render_welcome_slim_with_width_and_context,
    };
    use crate::model::{
        ClaudeUsageSummary, CodexUsageSummary, PublicIpSource, PublicIpSummary, SystemSummary,
        UsageAvailability,
    };

    fn sample_local_time_context() -> LocalTimeContext {
        LocalTimeContext {
            offset_minutes: 8 * 60,
            offset_label: "UTC+08:00".to_string(),
        }
    }

    #[test]
    fn welcome_slim_render_matches_wide_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output = render_welcome_slim_with_width_and_context(
            &snapshot,
            100,
            &sample_local_time_context(),
        );

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("welcome-slim-wide.txt"))
        );
    }

    #[test]
    fn welcome_slim_render_matches_narrow_snapshot() {
        let snapshot = sample_welcome_snapshot();
        let output =
            render_welcome_slim_with_width_and_context(&snapshot, 60, &sample_local_time_context());

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("welcome-slim-narrow.txt"))
        );
    }

    #[test]
    fn welcome_loading_render_matches_wide_snapshot() {
        let snapshot = sample_loading_welcome_snapshot();
        let output = render_welcome_loading_with_width_and_context(
            &snapshot,
            100,
            &sample_local_time_context(),
            0,
        );

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("welcome-loading-wide.txt"))
        );
    }

    #[test]
    fn welcome_loading_render_matches_narrow_snapshot() {
        let snapshot = sample_loading_welcome_snapshot();
        let output = render_welcome_loading_with_width_and_context(
            &snapshot,
            60,
            &sample_local_time_context(),
            0,
        );

        assert_eq!(
            normalize_snapshot(&output),
            normalize_snapshot(snapshot_text("welcome-loading-narrow.txt"))
        );
    }

    #[test]
    fn welcome_loading_render_advances_spinner_frames() {
        let snapshot = sample_loading_welcome_snapshot();
        let first = render_welcome_loading_with_width_and_context(
            &snapshot,
            100,
            &sample_local_time_context(),
            0,
        );
        let second = render_welcome_loading_with_width_and_context(
            &snapshot,
            100,
            &sample_local_time_context(),
            1,
        );

        assert!(first.contains("⠋ Loading..."));
        assert!(first.contains("⠋ Loading Copilot..."));
        assert!(second.contains("⠙ Loading..."));
        assert!(second.contains("⠙ Loading Copilot..."));
        assert_ne!(normalize_snapshot(&first), normalize_snapshot(&second));
    }

    #[test]
    fn loading_animation_interval_stays_within_target_range() {
        assert!((100..=200).contains(&ansi::LOADING_FRAME_INTERVAL_MILLIS));
    }

    fn sample_welcome_snapshot() -> WelcomeSnapshot {
        let fixture = fixture_map();

        WelcomeSnapshot {
            timestamp: fixture_value(&fixture, "timestamp"),
            user_label: fixture_value(&fixture, "user_label"),
            system: SystemSummary {
                host_label: fixture_value(&fixture, "system.host_label"),
                public_ip: PublicIpSummary {
                    address: fixture_value(&fixture, "system.public_ip.address"),
                    country_label: fixture_value(&fixture, "system.public_ip.country_label"),
                    source: PublicIpSource::Cache,
                    is_loading: false,
                },
                proxy_label: fixture_value(&fixture, "system.proxy_label"),
            },
            ai_tools: AiToolsSummary {
                claude_model: fixture_value(&fixture, "ai_tools.claude_model"),
                codex_model: fixture_value(&fixture, "ai_tools.codex_model"),
            },
            ai_usage: sample_ai_usage_summary(),
            copilot: sample_copilot_summary(),
        }
    }

    fn sample_loading_welcome_snapshot() -> WelcomeSnapshot {
        let mut snapshot = sample_welcome_snapshot();
        snapshot.system.public_ip = PublicIpSummary {
            address: "-".to_string(),
            country_label: String::new(),
            source: PublicIpSource::Unavailable,
            is_loading: true,
        };
        snapshot.copilot.is_loading = true;
        snapshot
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
                    resets_at_epoch_secs: None,
                },
                secondary_rate_limit: RateLimitWindow {
                    label: "Secondary",
                    used_percent: Some(12),
                    window_minutes: Some(10_080),
                    resets_at: "2026-03-30 00:00 UTC".to_string(),
                    resets_at_epoch_secs: None,
                },
                hint: "Run /status in Codex for current usage".to_string(),
            },
            warnings: Vec::new(),
        }
    }

    fn sample_copilot_summary() -> CopilotUsageSummary {
        CopilotUsageSummary {
            availability: UsageAvailability::Live,
            model: "claude-sonnet-4.6".to_string(),
            plan_type: "Individual".to_string(),
            is_loading: false,
            last_active_at: "2026-03-28 12:00 UTC".to_string(),
            total_requests: Some(585),
            last_24h_requests: Some(42),
            total_sessions: Some(25),
            remaining_percent: None,
            hint: "Run /usage in Copilot CLI for quota details".to_string(),
            quota: None,
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
            "welcome-slim-wide.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/welcome-slim-wide.txt"
            )),
            "welcome-slim-narrow.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/welcome-slim-narrow.txt"
            )),
            "welcome-loading-wide.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/welcome-loading-wide.txt"
            )),
            "welcome-loading-narrow.txt" => include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/snapshots/welcome-loading-narrow.txt"
            )),
            other => panic!("unknown snapshot: {other}"),
        }
    }

    fn normalize_snapshot(text: &str) -> &str {
        text.trim_end_matches('\n')
    }
}
