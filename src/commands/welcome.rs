use crate::collectors::system::collect_welcome_snapshot;
use crate::render::render_welcome;
use crate::shell;

pub fn execute() -> Result<(), String> {
    if !shell::can_render_welcome() {
        return Ok(());
    }

    let snapshot = collect_welcome_snapshot();
    print!("{}", render_welcome(&snapshot));
    Ok(())
}
