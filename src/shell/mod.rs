use std::env;
use std::io::{self, IsTerminal};

pub fn can_render_welcome() -> bool {
    io::stdout().is_terminal()
        && env::var("TERM")
            .map(|value| value != "dumb")
            .unwrap_or(true)
}
