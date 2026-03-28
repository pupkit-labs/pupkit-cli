use crate::collectors::ai_tools::collect_ai_tools_summary;
use crate::render::render_ai_tools_summary;

pub fn execute() -> Result<(), String> {
    let summary = collect_ai_tools_summary();
    print!("{}", render_ai_tools_summary(&summary));
    Ok(())
}
