use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const INSTALL_DIR: &str = ".local/bin";
const INSTALL_NAME: &str = "pup";
const MANAGED_START: &str = "# >>> pup-cli-start-rust install >>>";
const MANAGED_END: &str = "# <<< pup-cli-start-rust install <<<";

pub fn execute() -> Result<(), String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())?;
    let current_exe = env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let summary = install_into_home(home.as_path(), current_exe.as_path())?;
    print!("{}", render_install_summary(&summary));
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstallSummary {
    binary_path: PathBuf,
    shell_updates: Vec<PathBuf>,
    reload_hint: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShellHookMode {
    Interactive,
    PathOnly,
}

fn install_into_home(home: &Path, current_exe: &Path) -> Result<InstallSummary, String> {
    let install_dir = home.join(INSTALL_DIR);
    fs::create_dir_all(&install_dir)
        .map_err(|error| format!("failed to create {}: {error}", install_dir.display()))?;

    let binary_path = install_dir.join(INSTALL_NAME);
    install_binary(current_exe, &binary_path)?;

    let shell_targets = [
        (home.join(".zshrc"), ShellHookMode::Interactive),
        (home.join(".bashrc"), ShellHookMode::Interactive),
        (
            home.join(".config/fish/config.fish"),
            ShellHookMode::Interactive,
        ),
        (home.join(".zprofile"), ShellHookMode::PathOnly),
        (home.join(".bash_profile"), ShellHookMode::PathOnly),
        (home.join(".profile"), ShellHookMode::PathOnly),
    ];
    let mut shell_updates = Vec::new();

    for (target, mode) in shell_targets {
        upsert_shell_hook(&target, mode)?;
        shell_updates.push(target);
    }

    Ok(InstallSummary {
        binary_path,
        shell_updates,
        reload_hint: detect_reload_hint(),
    })
}

fn install_binary(source: &Path, target: &Path) -> Result<(), String> {
    if source != target {
        fs::copy(source, target).map_err(|error| {
            format!(
                "failed to copy {} to {}: {error}",
                source.display(),
                target.display()
            )
        })?;
    }

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(target)
            .map_err(|error| format!("failed to stat {}: {error}", target.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(target, permissions)
            .map_err(|error| format!("failed to chmod {}: {error}", target.display()))?;
    }

    Ok(())
}

fn upsert_shell_hook(path: &Path, mode: ShellHookMode) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid shell rc path: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;

    let existing = fs::read_to_string(path).unwrap_or_default();
    let block = managed_shell_block(path, mode);
    let updated = upsert_managed_block(&existing, &block);

    fs::write(path, updated).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn managed_shell_block(path: &Path, mode: ShellHookMode) -> String {
    if path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value == "config.fish")
    {
        match mode {
            ShellHookMode::Interactive => format!(
                "{MANAGED_START}\nif not contains -- $HOME/.local/bin $PATH\n    set -gx PATH $HOME/.local/bin $PATH\nend\nif status is-interactive\n    if command -q {INSTALL_NAME}\n        {INSTALL_NAME}\n    end\nend\n{MANAGED_END}\n"
            ),
            ShellHookMode::PathOnly => format!(
                "{MANAGED_START}\nif not contains -- $HOME/.local/bin $PATH\n    set -gx PATH $HOME/.local/bin $PATH\nend\n{MANAGED_END}\n"
            ),
        }
    } else {
        match mode {
            ShellHookMode::Interactive => format!(
                "{MANAGED_START}\ncase \":$PATH:\" in\n  *\":$HOME/.local/bin:\"*) ;;\n  *) export PATH=\"$HOME/.local/bin:$PATH\" ;;\nesac\ncase $- in\n  *i*)\n    if command -v {INSTALL_NAME} >/dev/null 2>&1; then\n      {INSTALL_NAME}\n    fi\n    ;;\nesac\n{MANAGED_END}\n"
            ),
            ShellHookMode::PathOnly => format!(
                "{MANAGED_START}\ncase \":$PATH:\" in\n  *\":$HOME/.local/bin:\"*) ;;\n  *) export PATH=\"$HOME/.local/bin:$PATH\" ;;\nesac\n{MANAGED_END}\n"
            ),
        }
    }
}

fn upsert_managed_block(existing: &str, block: &str) -> String {
    let trimmed_existing = existing.trim_end_matches('\n');
    if let Some(start) = trimmed_existing.find(MANAGED_START) {
        if let Some(end_relative) = trimmed_existing[start..].find(MANAGED_END) {
            let end = start + end_relative + MANAGED_END.len();
            let before = trimmed_existing[..start].trim_end_matches('\n');
            let after = trimmed_existing[end..].trim_start_matches('\n');
            return join_sections(before, block.trim_end_matches('\n'), after);
        }
    }

    join_sections(trimmed_existing, block.trim_end_matches('\n'), "")
}

fn join_sections(before: &str, middle: &str, after: &str) -> String {
    let mut sections = Vec::new();

    if !before.is_empty() {
        sections.push(before);
    }
    if !middle.is_empty() {
        sections.push(middle);
    }
    if !after.is_empty() {
        sections.push(after);
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!("{}\n", sections.join("\n\n"))
    }
}

fn render_install_summary(summary: &InstallSummary) -> String {
    let mut output = String::new();
    output.push_str("Install\n");
    output.push_str(&format!(
        "Installed binary: {}\n",
        summary.binary_path.display()
    ));
    output.push_str("Updated shell startup files:\n");
    for path in &summary.shell_updates {
        output.push_str(&format!("- {}\n", path.display()));
    }
    output.push_str("Auto-start is enabled for new interactive shells.\n");
    if let Some(hint) = &summary.reload_hint {
        output.push_str(&format!("Reload current shell: {hint}\n"));
    }
    output
}

fn detect_reload_hint() -> Option<String> {
    let shell_name = env::var("SHELL").ok()?;
    let shell_name = shell_name.rsplit('/').next()?;

    match shell_name {
        "zsh" => Some("source ~/.zshrc".to_string()),
        "bash" => Some("source ~/.bashrc".to_string()),
        "fish" => Some("source ~/.config/fish/config.fish".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        INSTALL_DIR, INSTALL_NAME, MANAGED_END, MANAGED_START, detect_reload_hint,
        install_into_home, upsert_managed_block,
    };

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

        fn read_file(&self, relative_path: &str) -> String {
            std::fs::read_to_string(self.path.join(relative_path)).unwrap()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn upsert_managed_block_replaces_existing_section() {
        let existing = format!("before\n{MANAGED_START}\nold\n{MANAGED_END}\nafter\n");
        let updated = upsert_managed_block(&existing, "new block\n");

        assert_eq!(updated, "before\n\nnew block\n\nafter\n");
    }

    #[test]
    fn install_writes_binary_and_shell_hooks() {
        let home = TestDir::new("install-home");
        let source = TestDir::new("install-source");
        source.write_file("bin/pup-cli-start-rust", "binary");

        let summary = install_into_home(
            home.path.as_path(),
            source.path.join("bin/pup-cli-start-rust").as_path(),
        )
        .unwrap();

        assert_eq!(
            summary.binary_path,
            home.path.join(INSTALL_DIR).join(INSTALL_NAME)
        );
        assert_eq!(home.read_file(".local/bin/pup"), "binary");
        assert!(home.read_file(".zshrc").contains(MANAGED_START));
        assert!(home.read_file(".bashrc").contains(MANAGED_START));
        assert!(
            home.read_file(".config/fish/config.fish")
                .contains(MANAGED_START)
        );
        assert!(home.read_file(".zprofile").contains(MANAGED_START));
        assert!(home.read_file(".bash_profile").contains(MANAGED_START));
        assert!(home.read_file(".profile").contains(MANAGED_START));
        assert!(summary.shell_updates.contains(&home.path.join(".zprofile")));
        assert!(
            summary
                .shell_updates
                .contains(&home.path.join(".bash_profile"))
        );
        assert!(summary.shell_updates.contains(&home.path.join(".profile")));
    }

    #[test]
    fn install_hook_is_idempotent() {
        let home = TestDir::new("install-repeat-home");
        let source = TestDir::new("install-repeat-source");
        source.write_file("bin/pup-cli-start-rust", "binary");
        let source_path = source.path.join("bin/pup-cli-start-rust");

        install_into_home(home.path.as_path(), source_path.as_path()).unwrap();
        install_into_home(home.path.as_path(), source_path.as_path()).unwrap();

        let zshrc = home.read_file(".zshrc");
        assert_eq!(zshrc.matches(MANAGED_START).count(), 1);
        assert_eq!(zshrc.matches(MANAGED_END).count(), 1);
    }

    #[test]
    fn reload_hint_matches_active_shell() {
        let original_shell = env::var_os("SHELL");

        unsafe {
            env::set_var("SHELL", "/bin/zsh");
        }
        assert_eq!(detect_reload_hint(), Some("source ~/.zshrc".to_string()));

        unsafe {
            env::set_var("SHELL", "/bin/bash");
        }
        assert_eq!(detect_reload_hint(), Some("source ~/.bashrc".to_string()));

        unsafe {
            env::set_var("SHELL", "/usr/bin/fish");
        }
        assert_eq!(
            detect_reload_hint(),
            Some("source ~/.config/fish/config.fish".to_string())
        );

        match original_shell {
            Some(value) => unsafe {
                env::set_var("SHELL", value);
            },
            None => unsafe {
                env::remove_var("SHELL");
            },
        }
    }
}
