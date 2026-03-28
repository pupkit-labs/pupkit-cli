use crate::collectors::services::collect_services;
use crate::render::render_services;

pub fn execute() -> Result<(), String> {
    let services = collect_services();
    print!("{}", render_services(&services));
    Ok(())
}
