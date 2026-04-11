use crate::daemon::PupkitDaemon;

pub fn execute() -> Result<(), String> {
    let daemon = PupkitDaemon::bootstrap();
    println!("{}", daemon.report());
    Ok(())
}
