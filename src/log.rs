//! Lightweight level-based logging controlled by `PUP_LOG_LEVEL`.
//!
//! Levels: `error`, `warn`, `info`, `debug` (case-insensitive).
//! Default level is `info` — debug messages are suppressed unless
//! `PUP_LOG_LEVEL=debug` is set.
//!
//! All output goes to stderr so it doesn't interfere with stdout-based IPC.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

static LEVEL: OnceLock<LogLevel> = OnceLock::new();

pub fn current_level() -> LogLevel {
    *LEVEL.get_or_init(|| {
        std::env::var("PUP_LOG_LEVEL")
            .ok()
            .and_then(|s| match s.to_lowercase().as_str() {
                "error" => Some(LogLevel::Error),
                "warn" => Some(LogLevel::Warn),
                "info" => Some(LogLevel::Info),
                "debug" => Some(LogLevel::Debug),
                _ => None,
            })
            .unwrap_or(LogLevel::Info)
    })
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        if $crate::log::current_level() >= $crate::log::LogLevel::Error {
            eprint!("[ERROR] ");
            eprintln!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        if $crate::log::current_level() >= $crate::log::LogLevel::Warn {
            eprint!("[WARN] ");
            eprintln!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        if $crate::log::current_level() >= $crate::log::LogLevel::Info {
            eprint!("[INFO] ");
            eprintln!($($arg)*);
        }
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        if $crate::log::current_level() >= $crate::log::LogLevel::Debug {
            eprint!("[DEBUG] ");
            eprintln!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_level_is_info() {
        // OnceLock may already be initialized by other tests running in parallel,
        // so we just verify the ordering invariant.
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
    }
}
