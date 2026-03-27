use std::env;
use std::io::{self, IsTerminal};

pub fn can_render_welcome() -> bool {
    io::stdout().is_terminal()
        && env::var("TERM")
            .map(|value| value != "dumb")
            .unwrap_or(true)
}

pub fn current_shell_label() -> String {
    if let Ok(version) = env::var("ZSH_VERSION") {
        return format!("zsh {version}");
    }

    if let Ok(path) = env::var("SHELL") {
        if let Some(name) = path.rsplit('/').next() {
            return name.to_string();
        }
    }

    "unknown-shell".to_string()
}
