use std::io::{self, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::collectors::copilot::finish_copilot_usage_summary;
use crate::collectors::system::collect_public_ip_summary;
use crate::collectors::{collect_fast_snapshot, collect_welcome_snapshot};
use crate::model::{CopilotUsageSummary, LoadingState, PublicIpSummary, WelcomeSnapshot};
use crate::render::{ansi, render_refresh, render_welcome_loading_frame, render_welcome_slim};
use crate::shell;

const MAX_LOAD_TIME_SECS: u64 = 3;

enum WelcomeUpdate {
    PublicIp(PublicIpSummary),
    Copilot(CopilotUsageSummary),
}

pub fn execute(explicit: bool) -> Result<(), String> {
    let can_render = shell::can_render_welcome();
    if !should_render_welcome(explicit, can_render) {
        return Ok(());
    }

    if !can_render {
        let snapshot = collect_welcome_snapshot();
        print!("{}", render_welcome_slim(&snapshot));
        return Ok(());
    }

    execute_with_loading()
}

fn execute_with_loading() -> Result<(), String> {
    let mut stdout = io::stdout();
    write!(stdout, "{}", ansi::HIDE_CURSOR)
        .map_err(|error| format!("failed to hide cursor: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("failed to flush welcome output: {error}"))?;

    let result = run_loading_flow(&mut stdout);

    let _ = write!(stdout, "{}", ansi::SHOW_CURSOR);
    let _ = stdout.flush();

    result
}

fn run_loading_flow(stdout: &mut io::Stdout) -> Result<(), String> {
    let mut snapshot = collect_fast_snapshot();
    let mut loading_frame = 0usize;
    let mut previous_output = render_snapshot(&snapshot, loading_frame);
    write!(stdout, "{}", previous_output)
        .map_err(|error| format!("failed to write welcome output: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("failed to flush welcome output: {error}"))?;

    let (tx, rx) = mpsc::channel();
    let copilot_summary = snapshot.copilot.clone();

    let tx_ip = tx.clone();
    std::thread::spawn(move || {
        let _ = tx_ip.send(WelcomeUpdate::PublicIp(collect_public_ip_summary()));
    });

    std::thread::spawn(move || {
        let summary = finish_copilot_usage_summary(copilot_summary);
        let _ = tx.send(WelcomeUpdate::Copilot(summary));
    });

    let deadline = Instant::now() + Duration::from_secs(MAX_LOAD_TIME_SECS);
    let loading_tick = Duration::from_millis(ansi::LOADING_FRAME_INTERVAL_MILLIS);
    let mut ip_state = LoadingState::Loading;
    let mut copilot_state = LoadingState::Loading;

    while ip_state.is_loading() || copilot_state.is_loading() {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if remaining.is_zero() {
            break;
        }

        let wait_time = remaining.min(loading_tick);

        match rx.recv_timeout(wait_time) {
            Ok(WelcomeUpdate::PublicIp(summary)) => {
                snapshot.system.public_ip = summary.clone();
                ip_state = LoadingState::Loaded(summary);
                previous_output =
                    refresh_snapshot(stdout, &previous_output, &snapshot, loading_frame)?;
            }
            Ok(WelcomeUpdate::Copilot(summary)) => {
                snapshot.copilot = summary.clone();
                copilot_state = LoadingState::Loaded(summary);
                previous_output =
                    refresh_snapshot(stdout, &previous_output, &snapshot, loading_frame)?;
            }
            Err(RecvTimeoutError::Timeout) => {
                loading_frame = loading_frame.wrapping_add(1);
                previous_output =
                    refresh_snapshot(stdout, &previous_output, &snapshot, loading_frame)?;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let mut needs_refresh = false;
    if snapshot.system.public_ip.is_loading {
        snapshot.system.public_ip.is_loading = false;
        needs_refresh = true;
    }
    if snapshot.copilot.is_loading {
        snapshot.copilot.is_loading = false;
        needs_refresh = true;
    }

    if needs_refresh {
        previous_output = refresh_snapshot(stdout, &previous_output, &snapshot, loading_frame)?;
    }

    let _ = previous_output;
    Ok(())
}

fn render_snapshot(snapshot: &WelcomeSnapshot, loading_frame: usize) -> String {
    if snapshot.system.public_ip.is_loading || snapshot.copilot.is_loading {
        render_welcome_loading_frame(snapshot, loading_frame)
    } else {
        render_welcome_slim(snapshot)
    }
}

fn refresh_snapshot(
    stdout: &mut io::Stdout,
    previous_output: &str,
    snapshot: &WelcomeSnapshot,
    loading_frame: usize,
) -> Result<String, String> {
    let next_output = render_snapshot(snapshot, loading_frame);
    write!(stdout, "{}", render_refresh(previous_output, &next_output))
        .map_err(|error| format!("failed to refresh welcome output: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("failed to flush welcome output: {error}"))?;
    Ok(next_output)
}

fn should_render_welcome(explicit: bool, can_render: bool) -> bool {
    explicit || can_render
}

#[cfg(test)]
mod tests {
    use super::should_render_welcome;

    #[test]
    fn explicit_welcome_renders_without_tty() {
        assert!(should_render_welcome(true, false));
    }

    #[test]
    fn implicit_welcome_keeps_tty_gate() {
        assert!(!should_render_welcome(false, false));
        assert!(should_render_welcome(false, true));
    }
}
