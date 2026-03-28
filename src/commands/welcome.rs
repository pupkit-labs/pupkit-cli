use crate::collectors::system::collect_welcome_snapshot;
use crate::render::render_welcome;
use crate::shell;

pub fn execute(explicit: bool) -> Result<(), String> {
    if !should_render_welcome(explicit, shell::can_render_welcome()) {
        return Ok(());
    }

    let snapshot = collect_welcome_snapshot();
    print!("{}", render_welcome(&snapshot));
    Ok(())
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
