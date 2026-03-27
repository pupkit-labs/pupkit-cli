use std::env;
use std::io::{self, IsTerminal};
use std::process::Command;

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
            if name == "zsh" {
                if let Some(version) = run_command("zsh", &["--version"]) {
                    return version;
                }
            }
            return name.to_string();
        }
    }

    "unknown-shell".to_string()
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
