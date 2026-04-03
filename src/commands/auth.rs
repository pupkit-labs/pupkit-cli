use crate::collectors::copilot::run_github_auth_flow;

pub fn execute() -> Result<(), String> {
    let token_path = run_github_auth_flow()?;
    println!("GitHub token saved to {}", token_path.display());
    Ok(())
}
