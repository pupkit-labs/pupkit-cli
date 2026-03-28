use crate::collectors::ai_usage::collect_ai_usage_summary;
use crate::render::render_ai_usage_summary;

pub fn execute() -> Result<(), String> {
    let summary = collect_ai_usage_summary();
    print!("{}", render_ai_usage_summary(&summary));
    Ok(())
}
