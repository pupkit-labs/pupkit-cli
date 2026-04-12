use crate::daemon::PupkitDaemon;

pub fn execute() -> Result<(), String> {
    let mut daemon = PupkitDaemon::bootstrap();
    let snapshot = daemon.state_snapshot();
    let json = serde_json::to_string_pretty(&snapshot)
        .map_err(|error| format!("failed to serialize monitor snapshot: {error}"))?;
    println!("{json}");
    Ok(())
}
