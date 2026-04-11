mod action;
mod auth;
mod bridge;
mod daemon;
mod hook;
mod monitor;
mod update;
mod welcome;

use action::ActionCommand;
use bridge::BridgeSource;
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
        Command::Bridge { source } => bridge::execute(source),
        Command::Action(command) => action::execute(command),
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
    Bridge { source: BridgeSource },
    Action(ActionCommand),
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match args.get(1).map(String::as_str) {
        None => Ok(Command::Welcome { explicit: false }),
        Some("welcome") if args.len() == 2 => Ok(Command::Welcome { explicit: true }),
        Some("welcome") => Err(format!(
            "welcome does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        Some("auth") if args.len() == 2 => Ok(Command::Auth),
        Some("auth") => Err(format!(
            "auth does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        Some("update") if args.len() == 2 => Ok(Command::Update),
        Some("update") => Err(format!(
            "update does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        Some("daemon") if args.len() == 2 => Ok(Command::Daemon),
        Some("daemon") => Err(format!(
            "daemon does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        Some("monitor") if args.len() == 2 => Ok(Command::Monitor),
        Some("monitor") => Err(format!(
            "monitor does not take additional arguments

{}",
            usage_text(&program_name(args))
        )),
        Some("hook") => parse_hook_command(args),
        Some("bridge") => parse_bridge_command(args),
        Some("action") => parse_action_command(args),
        Some(other) => Err(format!(
            "unsupported command: {other}

{}",
            usage_text(&program_name(args))
        )),
    }
}

fn parse_hook_command(args: &[String]) -> Result<Command, String> {
    match (args.get(2).map(String::as_str), args.len()) {
        (Some("install"), 3) => Ok(Command::HookInstall),
        (Some("doctor"), 3) => Ok(Command::HookDoctor),
        _ => Err(format!(
            "hook requires one of: install, doctor

{}",
            usage_text(&program_name(args))
        )),
    }
}

fn parse_bridge_command(args: &[String]) -> Result<Command, String> {
    match (args.get(2).map(String::as_str), args.len()) {
        (Some("claude"), 3) => Ok(Command::Bridge {
            source: BridgeSource::Claude,
        }),
        (Some("codex"), 3) => Ok(Command::Bridge {
            source: BridgeSource::Codex,
        }),
        _ => Err(format!(
            "bridge requires one of: claude, codex

{}",
            usage_text(&program_name(args))
        )),
    }
}

fn parse_action_command(args: &[String]) -> Result<Command, String> {
    match (
        args.get(2).map(String::as_str),
        args.get(3),
        args.get(4),
        args.len(),
    ) {
        (Some("approve"), Some(request_id), None, 4) => {
            Ok(Command::Action(ActionCommand::Approve {
                request_id: request_id.clone(),
                always: false,
            }))
        }
        (Some("approve-always"), Some(request_id), None, 4) => {
            Ok(Command::Action(ActionCommand::Approve {
                request_id: request_id.clone(),
                always: true,
            }))
        }
        (Some("deny"), Some(request_id), None, 4) => Ok(Command::Action(ActionCommand::Deny {
            request_id: request_id.clone(),
        })),
        (Some("answer-option"), Some(request_id), Some(option_id), 5) => {
            Ok(Command::Action(ActionCommand::AnswerOption {
                request_id: request_id.clone(),
                option_id: option_id.clone(),
            }))
        }
        (Some("answer-text"), Some(request_id), Some(text), 5) => {
            Ok(Command::Action(ActionCommand::AnswerText {
                request_id: request_id.clone(),
                text: text.clone(),
            }))
        }
        _ => Err(format!(
            "action requires approve|approve-always|deny|answer-option|answer-text

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
  {program} bridge [claude|codex] < input.json
  {program} action approve <request_id>
  {program} action approve-always <request_id>
  {program} action deny <request_id>
  {program} action answer-option <request_id> <option_id>
  {program} action answer-text <request_id> <text>
"
    )
}

#[cfg(test)]
mod tests {
    use super::{ActionCommand, BridgeSource, Command, parse_command};

    #[test]
    fn defaults_to_implicit_welcome_when_no_command_is_passed() {
        let args = vec!["pup".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Welcome { explicit: false }
        );
    }

    #[test]
    fn parses_bridge_claude_command() {
        let args = vec![
            "pup".to_string(),
            "bridge".to_string(),
            "claude".to_string(),
        ];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Bridge {
                source: BridgeSource::Claude
            }
        );
    }

    #[test]
    fn parses_bridge_codex_command() {
        let args = vec!["pup".to_string(), "bridge".to_string(), "codex".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Bridge {
                source: BridgeSource::Codex
            }
        );
    }

    #[test]
    fn parses_action_approve_command() {
        let args = vec![
            "pup".to_string(),
            "action".to_string(),
            "approve".to_string(),
            "req-1".to_string(),
        ];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Action(ActionCommand::Approve {
                request_id: "req-1".to_string(),
                always: false
            })
        );
    }

    #[test]
    fn parses_action_answer_option_command() {
        let args = vec![
            "pup".to_string(),
            "action".to_string(),
            "answer-option".to_string(),
            "req-1".to_string(),
            "yes".to_string(),
        ];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Action(ActionCommand::AnswerOption {
                request_id: "req-1".to_string(),
                option_id: "yes".to_string()
            })
        );
    }
}
