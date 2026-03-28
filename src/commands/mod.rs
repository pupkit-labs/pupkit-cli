mod ai_tools;
mod services;
mod system_summary;
mod welcome;

use crate::model::AppCommand;

pub fn run(args: Vec<String>) -> Result<(), String> {
    let command = parse_command(&args)?;
    let explicit_welcome = matches!(args.get(1).map(String::as_str), Some("welcome"));

    match command {
        AppCommand::Welcome => welcome::execute(explicit_welcome),
        AppCommand::SystemSummary => system_summary::execute(),
        AppCommand::AiTools => ai_tools::execute(),
        AppCommand::Services => services::execute(),
        AppCommand::Help => {
            print!("{}", help_text(&program_name(&args)));
            Ok(())
        }
        AppCommand::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

fn parse_command(args: &[String]) -> Result<AppCommand, String> {
    let command = args.get(1).map(String::as_str);

    match command {
        None => Ok(AppCommand::Welcome),
        Some("welcome") => Ok(AppCommand::Welcome),
        Some("system-summary") => Ok(AppCommand::SystemSummary),
        Some("ai-tools") => Ok(AppCommand::AiTools),
        Some("services") => Ok(AppCommand::Services),
        Some("help") | Some("-h") | Some("--help") => Ok(AppCommand::Help),
        Some("version") | Some("-V") | Some("--version") => Ok(AppCommand::Version),
        Some(other) => Err(format!(
            "unknown command: {other}\n\n{}",
            help_text(&program_name(args))
        )),
    }
}

fn program_name(args: &[String]) -> String {
    args.first()
        .map(String::as_str)
        .unwrap_or("pup-cli-start-rust")
        .to_string()
}

fn help_text(program: &str) -> String {
    format!(
        "\
Usage:
  {program} [command]

Commands:
  welcome         Render the current local welcome screen
  system-summary  Print the current local system summary
  ai-tools        Print the local Claude and Codex summary
  services        Print the current local services summary
  help            Show this help text
  version         Show package version
"
    )
}

#[cfg(test)]
mod tests {
    use crate::model::AppCommand;

    use super::parse_command;

    #[test]
    fn defaults_to_welcome_when_no_command_is_passed() {
        let args = vec!["pup".to_string()];
        assert_eq!(parse_command(&args).unwrap(), AppCommand::Welcome);
    }

    #[test]
    fn parses_system_summary_command() {
        let args = vec!["pup".to_string(), "system-summary".to_string()];
        assert_eq!(parse_command(&args).unwrap(), AppCommand::SystemSummary);
    }

    #[test]
    fn parses_ai_tools_command() {
        let args = vec!["pup".to_string(), "ai-tools".to_string()];
        assert_eq!(parse_command(&args).unwrap(), AppCommand::AiTools);
    }

    #[test]
    fn parses_services_command() {
        let args = vec!["pup".to_string(), "services".to_string()];
        assert_eq!(parse_command(&args).unwrap(), AppCommand::Services);
    }

    #[test]
    fn rejects_unknown_commands() {
        let args = vec!["pup".to_string(), "unknown".to_string()];
        let error = parse_command(&args).unwrap_err();
        assert!(error.contains("unknown command"));
    }
}
