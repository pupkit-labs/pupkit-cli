mod auth;
mod update;
mod welcome;

pub fn run(args: Vec<String>) -> Result<(), String> {
    match parse_command(&args)? {
        Command::Welcome { explicit } => welcome::execute(explicit),
        Command::Auth => auth::execute(),
        Command::Update => update::execute(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Welcome { explicit: bool },
    Auth,
    Update,
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match args.get(1).map(String::as_str) {
        None => Ok(Command::Welcome { explicit: false }),
        Some("welcome") if args.len() == 2 => Ok(Command::Welcome { explicit: true }),
        Some("welcome") => Err(format!(
            "welcome does not take additional arguments\n\n{}",
            usage_text(&program_name(args))
        )),
        Some("auth") if args.len() == 2 => Ok(Command::Auth),
        Some("auth") => Err(format!(
            "auth does not take additional arguments\n\n{}",
            usage_text(&program_name(args))
        )),
        Some("update") if args.len() == 2 => Ok(Command::Update),
        Some("update") => Err(format!(
            "update does not take additional arguments\n\n{}",
            usage_text(&program_name(args))
        )),
        Some(other) => Err(format!(
            "unsupported command: {other}\n\n{}",
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
  {program} [welcome|auth|update]
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
    fn rejects_additional_welcome_arguments() {
        let args = vec![
            "pup".to_string(),
            "welcome".to_string(),
            "--extra".to_string(),
        ];
        let error = parse_command(&args).unwrap_err();

        assert!(error.contains("welcome does not take additional arguments"));
    }

    #[test]
    fn rejects_additional_auth_arguments() {
        let args = vec!["pup".to_string(), "auth".to_string(), "--extra".to_string()];
        let error = parse_command(&args).unwrap_err();

        assert!(error.contains("auth does not take additional arguments"));
    }

    #[test]
    fn rejects_additional_update_arguments() {
        let args = vec![
            "pup".to_string(),
            "update".to_string(),
            "--extra".to_string(),
        ];
        let error = parse_command(&args).unwrap_err();

        assert!(error.contains("update does not take additional arguments"));
    }

    #[test]
    fn rejects_unsupported_commands() {
        let args = vec!["pup".to_string(), "unknown".to_string()];
        let error = parse_command(&args).unwrap_err();

        assert!(error.contains("unsupported command"));
    }
}
