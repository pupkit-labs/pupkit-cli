use crate::collectors::services::collect_services;
use crate::collectors::system::collect_system_summary;
use crate::collectors::ai_tools::collect_ai_tools_summary;
use crate::collectors::ai_usage::collect_ai_usage_summary;
use crate::render::render_details;

pub fn execute() -> Result<(), String> {
    let system = collect_system_summary();
    let ai_tools = collect_ai_tools_summary();
    let ai_usage = collect_ai_usage_summary();
    let services = collect_services();
    print!("{}", render_details(&system, &ai_tools, &ai_usage, &services));
    Ok(())
}
