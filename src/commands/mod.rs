mod action;
mod auth;
mod bridge;
mod daemon;
mod hook;
mod monitor;
mod service;
mod shell;
mod update;
mod welcome;

use action::ActionCommand;
use bridge::BridgeSource;
use daemon::DaemonCommand;
use hook::HookCommand;
use service::ServiceCommand;
use shell::ShellCommand;

pub fn run(args: Vec<String>) -> Result<(), String> {
    match parse_command(&args)? {
        Command::Welcome { explicit } => welcome::execute(explicit),
        Command::Auth => auth::execute(),
        Command::Update => update::execute(),
        Command::Daemon(cmd) => daemon::execute(cmd),
        Command::Monitor => monitor::execute(),
        Command::HookInstall => hook::execute(HookCommand::Install),
        Command::HookDoctor => hook::execute(HookCommand::Doctor),
        Command::Bridge { source } => bridge::execute(source),
        Command::Action(command) => action::execute(command),
        Command::Shell(cmd) => shell::execute(cmd),
        Command::Service(cmd) => service::execute(cmd),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Welcome { explicit: bool },
    Auth,
    Update,
    Daemon(DaemonCommand),
    Monitor,
    HookInstall,
    HookDoctor,
    Bridge { source: BridgeSource },
    Action(ActionCommand),
    Shell(ShellCommand),
    Service(ServiceCommand),
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
        Some("daemon") => parse_daemon_command(args),
        Some("shell") => parse_shell_command(args),
        Some("start") if args.len() == 2 => Ok(Command::Service(ServiceCommand::Start)),
        Some("stop") if args.len() == 2 => Ok(Command::Service(ServiceCommand::Stop)),
        Some("restart") if args.len() == 2 => Ok(Command::Service(ServiceCommand::Restart)),
        Some("status") if args.len() == 2 => Ok(Command::Service(ServiceCommand::Status)),
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

fn parse_daemon_command(args: &[String]) -> Result<Command, String> {
    match (args.get(2).map(String::as_str), args.len()) {
        (None, _) | (Some("start"), 3) => Ok(Command::Daemon(DaemonCommand::Start)),
        (Some("stop"), 3) => Ok(Command::Daemon(DaemonCommand::Stop)),
        (Some("restart"), 3) => Ok(Command::Daemon(DaemonCommand::Restart)),
        (Some("status"), 3) => Ok(Command::Daemon(DaemonCommand::Status)),
        _ => Err(format!(
            "daemon requires one of: start, stop, restart, status\n\n{}",
            usage_text(&program_name(args))
        )),
    }
}

fn parse_shell_command(args: &[String]) -> Result<Command, String> {
    match (args.get(2).map(String::as_str), args.len()) {
        (None, _) | (Some("start"), 3) => Ok(Command::Shell(ShellCommand::Start)),
        (Some("stop"), 3) => Ok(Command::Shell(ShellCommand::Stop)),
        (Some("restart"), 3) => Ok(Command::Shell(ShellCommand::Restart)),
        (Some("status"), 3) => Ok(Command::Shell(ShellCommand::Status)),
        _ => Err(format!(
            "shell requires one of: start, stop, restart, status\n\n{}",
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
  {program} [welcome|auth|update|monitor]
  {program} start|stop|restart|status
  {program} daemon [start|stop|restart|status]
  {program} shell [start|stop|restart|status]
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
    use super::{ActionCommand, BridgeSource, Command, DaemonCommand, ServiceCommand, ShellCommand, parse_command};

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

    #[test]
    fn parses_daemon_start_command() {
        let args = vec!["pup".to_string(), "daemon".to_string(), "start".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Daemon(DaemonCommand::Start)
        );
    }

    #[test]
    fn daemon_without_subcommand_defaults_to_start() {
        let args = vec!["pup".to_string(), "daemon".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Daemon(DaemonCommand::Start)
        );
    }

    #[test]
    fn parses_daemon_stop_command() {
        let args = vec!["pup".to_string(), "daemon".to_string(), "stop".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Daemon(DaemonCommand::Stop)
        );
    }

    #[test]
    fn parses_daemon_status_command() {
        let args = vec!["pup".to_string(), "daemon".to_string(), "status".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Daemon(DaemonCommand::Status)
        );
    }

    #[test]
    fn parses_shell_start_command() {
        let args = vec!["pup".to_string(), "shell".to_string(), "start".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Shell(ShellCommand::Start)
        );
    }

    #[test]
    fn shell_without_subcommand_defaults_to_start() {
        let args = vec!["pup".to_string(), "shell".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Shell(ShellCommand::Start)
        );
    }

    #[test]
    fn parses_shell_stop_command() {
        let args = vec!["pup".to_string(), "shell".to_string(), "stop".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Shell(ShellCommand::Stop)
        );
    }

    #[test]
    fn parses_top_level_start() {
        let args = vec!["pup".to_string(), "start".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Service(ServiceCommand::Start)
        );
    }

    #[test]
    fn parses_top_level_stop() {
        let args = vec!["pup".to_string(), "stop".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Service(ServiceCommand::Stop)
        );
    }

    #[test]
    fn parses_top_level_restart() {
        let args = vec!["pup".to_string(), "restart".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Service(ServiceCommand::Restart)
        );
    }

    #[test]
    fn parses_top_level_status() {
        let args = vec!["pup".to_string(), "status".to_string()];
        assert_eq!(
            parse_command(&args).unwrap(),
            Command::Service(ServiceCommand::Status)
        );
    }
}
