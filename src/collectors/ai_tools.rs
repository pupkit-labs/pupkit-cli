use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::model::AiToolsSummary;

const DEFAULT_CLAUDE_MODEL: &str = "claude-sonnet-4-6";
const DEFAULT_CODEX_MODEL: &str = "gpt-5.4";
const EMPTY_SKILLS_LABEL: &str = "(none)";

pub fn collect_ai_tools_summary() -> AiToolsSummary {
    let home = env::var_os("HOME").map(PathBuf::from);
    collect_ai_tools_summary_with_home(home.as_deref())
}

fn collect_ai_tools_summary_with_home(home: Option<&Path>) -> AiToolsSummary {
    AiToolsSummary {
        claude_model: detect_claude_model(home),
        claude_skills: detect_claude_skills(home),
        codex_model: detect_codex_model(home),
        codex_skills: detect_codex_skills(home),
    }
}

fn detect_claude_model(home: Option<&Path>) -> String {
    read_home_file(home, ".claude/settings.json")
        .and_then(|content| parse_json_string_value(&content, "ANTHROPIC_MODEL"))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CLAUDE_MODEL.to_string())
}

fn detect_codex_model(home: Option<&Path>) -> String {
    read_home_file(home, ".codex/config.toml")
        .and_then(|content| parse_toml_model(&content))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CODEX_MODEL.to_string())
}

fn detect_claude_skills(home: Option<&Path>) -> String {
    collect_skill_names(home, &[".claude/settings/skills", ".claude/skills"])
}

fn detect_codex_skills(home: Option<&Path>) -> String {
    collect_skill_names(home, &[".codex/skills"])
}

fn read_home_file(home: Option<&Path>, relative_path: &str) -> Option<String> {
    let home = home?;
    fs::read_to_string(home.join(relative_path)).ok()
}

fn collect_skill_names(home: Option<&Path>, relative_paths: &[&str]) -> String {
    let Some(home) = home else {
        return EMPTY_SKILLS_LABEL.to_string();
    };

    let mut names = BTreeSet::new();

    for relative_path in relative_paths {
        let directory = home.join(relative_path);
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };

        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().trim().to_string();
            if name.is_empty() || name.starts_with('.') {
                continue;
            }

            names.insert(name);
        }
    }

    if names.is_empty() {
        EMPTY_SKILLS_LABEL.to_string()
    } else {
        names.into_iter().collect::<Vec<_>>().join(", ")
    }
}

fn parse_json_string_value(content: &str, key: &str) -> Option<String> {
    let key_pattern = format!("\"{key}\"");
    let key_start = content.find(&key_pattern)?;
    let rest = &content[key_start + key_pattern.len()..];
    let colon_index = rest.find(':')?;
    parse_quoted_string(rest[colon_index + 1..].trim_start())
}

fn parse_toml_model(content: &str) -> Option<String> {
    for raw_line in content.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let (key, value) = line.split_once('=')?;
        if key.trim() != "model" {
            continue;
        }

        return parse_quoted_string(value.trim());
    }

    None
}

fn parse_quoted_string(input: &str) -> Option<String> {
    let mut chars = input.chars();
    if chars.next()? != '"' {
        return None;
    }

    let mut output = String::new();

    while let Some(character) = chars.next() {
        match character {
            '"' => return Some(output),
            '\\' => {
                let escaped = chars.next()?;
                match escaped {
                    '"' | '\\' | '/' => output.push(escaped),
                    'b' => output.push('\u{0008}'),
                    'f' => output.push('\u{000C}'),
                    'n' => output.push('\n'),
                    'r' => output.push('\r'),
                    't' => output.push('\t'),
                    'u' => {
                        let mut digits = String::new();
                        for _ in 0..4 {
                            digits.push(chars.next()?);
                        }
                        let codepoint = u32::from_str_radix(&digits, 16).ok()?;
                        output.push(char::from_u32(codepoint)?);
                    }
                    other => output.push(other),
                }
            }
            other => output.push(other),
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::collect_ai_tools_summary_with_home;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "pup-cli-start-rust-{prefix}-{}-{timestamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn write_file(&self, relative_path: &str, content: &str) {
            let path = self.path.join(relative_path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }

        fn create_dir(&self, relative_path: &str) {
            std::fs::create_dir_all(self.path.join(relative_path)).unwrap();
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn collects_models_and_skills_from_home_layout() {
        let home = TestDir::new("ai-tools");
        home.write_file(
            ".claude/settings.json",
            r#"{
  "env": {
    "ANTHROPIC_MODEL": "MiniMax-M2.5"
  }
}"#,
        );
        home.write_file(
            ".codex/config.toml",
            r#"
model = "gpt-5.4-mini"
model_reasoning_effort = "high"
"#,
        );
        home.create_dir(".claude/settings/skills/research");
        home.create_dir(".claude/skills/ship");
        home.create_dir(".claude/skills/.hidden");
        home.create_dir(".claude/skills/research");
        home.create_dir(".codex/skills/review");
        home.create_dir(".codex/skills/.system");

        let summary = collect_ai_tools_summary_with_home(Some(home.path.as_path()));

        assert_eq!(summary.claude_model, "MiniMax-M2.5");
        assert_eq!(summary.codex_model, "gpt-5.4-mini");
        assert_eq!(summary.claude_skills, "research, ship");
        assert_eq!(summary.codex_skills, "review");
    }

    #[test]
    fn falls_back_to_defaults_when_files_are_missing() {
        let home = TestDir::new("ai-tools-defaults");

        let summary = collect_ai_tools_summary_with_home(Some(home.path.as_path()));

        assert_eq!(summary.claude_model, "claude-sonnet-4-6");
        assert_eq!(summary.codex_model, "gpt-5.4");
        assert_eq!(summary.claude_skills, "(none)");
        assert_eq!(summary.codex_skills, "(none)");
    }
}
