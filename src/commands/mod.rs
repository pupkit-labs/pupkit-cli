mod welcome;

pub fn run(args: Vec<String>) -> Result<(), String> {
    let explicit_welcome = resolve_welcome_mode(&args)?;
    welcome::execute(explicit_welcome)
}

fn resolve_welcome_mode(args: &[String]) -> Result<bool, String> {
    match args.get(1).map(String::as_str) {
        None => Ok(false),
        Some("welcome") if args.len() == 2 => Ok(true),
        Some("welcome") => Err(format!(
            "welcome does not take additional arguments\n\n{}",
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
        .unwrap_or("pup-cli-start-rust")
        .to_string()
}

fn usage_text(program: &str) -> String {
    format!(
        "\
Usage:
  {program} [welcome]
"
    )
}

#[cfg(test)]
mod tests {
    use super::resolve_welcome_mode;

    #[test]
    fn defaults_to_implicit_welcome_when_no_command_is_passed() {
        let args = vec!["pup".to_string()];
        assert!(!resolve_welcome_mode(&args).unwrap());
    }

    #[test]
    fn parses_explicit_welcome_command() {
        let args = vec!["pup".to_string(), "welcome".to_string()];
        assert!(resolve_welcome_mode(&args).unwrap());
    }

    #[test]
    fn rejects_additional_welcome_arguments() {
        let args = vec![
            "pup".to_string(),
            "welcome".to_string(),
            "--extra".to_string(),
        ];
        let error = resolve_welcome_mode(&args).unwrap_err();

        assert!(error.contains("welcome does not take additional arguments"));
    }

    #[test]
    fn rejects_unsupported_commands() {
        let args = vec!["pup".to_string(), "unknown".to_string()];
        let error = resolve_welcome_mode(&args).unwrap_err();

        assert!(error.contains("unsupported command"));
    }
}
