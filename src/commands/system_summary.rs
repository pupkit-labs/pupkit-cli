use crate::collectors::system::collect_system_summary;
use crate::render::render_system_summary;

pub fn execute() -> Result<(), String> {
    let summary = collect_system_summary();
    print!("{}", render_system_summary(&summary));
    Ok(())
}
