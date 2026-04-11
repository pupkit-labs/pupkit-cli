mod auth;
mod daemon;
mod hook;
mod monitor;
mod update;
mod welcome;

use hook::HookCommand;

pub fn run(args: Vec<String>) -> Result<(), String> {
    match parse_command(&args)? {
        Command::Welcome { explicit } => welcome::execute(explicit),
        Command::Auth => auth::execute(),
        Command::Update => update::execute(),
        Command::Daemon => daemon::execute(),
        Command::Monitor => monitor::execute(),
        Command::HookInstall => hook::execute(HookCommand::Install),
        Command::HookDoctor => hook::execute(HookCommand::Doctor),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Welcome { explicit: bool },
    Auth,
    Update,
    Daemon,
    Monitor,
    HookInstall,
    HookDoctor,
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match (
        args.get(1).map(String::as_str),
        args.get(2).map(String::as_str),
        args.len(),
    ) {
        (None, _, _) => Ok(Command::Welcome { explicit: false }),
        (Some("welcome"), None, 2) => Ok(Command::Welcome { explicit: true }),
        (Some("welcome"), _, _) => Err(format!(
            "welcome does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        (Some("auth"), None, 2) => Ok(Command::Auth),
        (Some("auth"), _, _) => Err(format!(
            "auth does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        (Some("update"), None, 2) => Ok(Command::Update),
        (Some("update"), _, _) => Err(format!(
            "update does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        (Some("daemon"), None, 2) => Ok(Command::Daemon),
        (Some("daemon"), _, _) => Err(format!(
            "daemon does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        (Some("monitor"), None, 2) => Ok(Command::Monitor),
        (Some("monitor"), _, _) => Err(format!(
            "monitor does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        (Some("hook"), Some("install"), 3) => Ok(Command::HookInstall),
        (Some("hook"), Some("doctor"), 3) => Ok(Command::HookDoctor),
        (Some("hook"), _, _) => Err(format!(
            "hook requires one of: install, doctor

{}",
            usage_text(&program_name(args))
        )),
        (Some(other), _, _) => Err(format!(
            "unsupported command: {other}

{}",
            usage_text(&program_name(args))
        )),
    }
}

fn program_name(args: &[String]) -> String {
    args.first()
        .map(String::as_str)
        .unwrap_or("pupkit")
        .to_string()
}

fn usage_text(program: &str) -> String {
    format!(
        "\
Usage:
  {program} [welcome|auth|update|daemon|monitor]
  {program} hook [install|doctor]
"
    )
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_command};

    #[test]
    fn defaults_to_implicit_welcome_when_no_command_is_passed() {
        let args = vec!["pup".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Welcome { explicit: false }
        );
    }

    #[test]
    fn parses_explicit_welcome_command() {
        let args = vec!["pup".to_string(), "welcome".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Welcome { explicit: true }
        );
    }

    #[test]
    fn parses_auth_command() {
        let args = vec!["pup".to_string(), "auth".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::Auth);
    }

    #[test]
    fn parses_update_command() {
        let args = vec!["pup".to_string(), "update".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::Update);
    }

    #[test]
    fn parses_daemon_command() {
        let args = vec!["pup".to_string(), "daemon".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::Daemon);
    }

    #[test]
    fn parses_monitor_command() {
        let args = vec!["pup".to_string(), "monitor".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::Monitor);
    }

    #[test]
    fn parses_hook_install_command() {
        let args = vec!["pup".to_string(), "hook".to_string(), "install".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::HookInstall);
    }

    #[test]
    fn parses_hook_doctor_command() {
        let args = vec!["pup".to_string(), "hook".to_string(), "doctor".to_string()];
        assert_eq!(parse_command(&args).unwrap(), Command::HookDoctor);
    }
}
