use crate::collectors::system::collect_system_summary;
use crate::render::render_welcome;
use crate::shell;

pub fn execute() -> Result<(), String> {
    if !shell::can_render_welcome() {
        return Ok(());
    }

    let summary = collect_system_summary();
    print!("{}", render_welcome(&summary));
    Ok(())
}
