use crate::collectors::ai_tools::collect_ai_tools_summary;
use crate::collectors::ai_usage::collect_ai_usage_summary;
use crate::render::{render_ai_skills_summary, render_ai_tools_summary};

pub fn execute(args: &[String]) -> Result<(), String> {
    match parse_mode(args)? {
        AiToolsMode::Overview => {
            let summary = collect_ai_tools_summary();
            let usage = collect_ai_usage_summary();
            print!("{}", render_ai_tools_summary(&summary, &usage));
            Ok(())
        }
        AiToolsMode::Skills => {
            let summary = collect_ai_tools_summary();
            print!("{}", render_ai_skills_summary(&summary));
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AiToolsMode {
    Overview,
    Skills,
}

fn parse_mode(args: &[String]) -> Result<AiToolsMode, String> {
    match args {
        [] => Ok(AiToolsMode::Overview),
        [flag] if matches!(flag.as_str(), "--skills" | "skills") => Ok(AiToolsMode::Skills),
        [flag] => Err(format!(
            "unknown ai-tools argument: {flag}\n\nUsage:\n  ai-tools\n  ai-tools --skills\n"
        )),
        _ => Err(
            "too many ai-tools arguments\n\nUsage:\n  ai-tools\n  ai-tools --skills\n".to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{AiToolsMode, parse_mode};

    #[test]
    fn defaults_to_overview_mode() {
        assert_eq!(parse_mode(&[]).unwrap(), AiToolsMode::Overview);
    }

    #[test]
    fn accepts_skills_flag() {
        assert_eq!(
            parse_mode(&["--skills".to_string()]).unwrap(),
            AiToolsMode::Skills
        );
        assert_eq!(
            parse_mode(&["skills".to_string()]).unwrap(),
            AiToolsMode::Skills
        );
    }

    #[test]
    fn rejects_unknown_arguments() {
        let error = parse_mode(&["--unknown".to_string()]).unwrap_err();
        assert!(error.contains("unknown ai-tools argument"));
    }
}
