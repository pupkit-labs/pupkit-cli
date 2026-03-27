pub mod collectors;
pub mod commands;
pub mod model;
pub mod render;
pub mod shell;

pub fn run(args: Vec<String>) -> Result<(), String> {
    commands::run(args)
}
