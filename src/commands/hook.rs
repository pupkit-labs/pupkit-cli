use std::env;
use std::fs;
use std::path::PathBuf;

pub enum HookCommand {
    Install,
    Doctor,
}

pub fn execute(command: HookCommand) -> Result<(), String> {
    match command {
        HookCommand::Install => install_hook_templates(),
        HookCommand::Doctor => doctor_hook_templates(),
    }
}

fn hooks_dir() -> Result<PathBuf, String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set; cannot determine pupkit hook dir".to_string())?;
    Ok(home.join(".local/share/pupkit/hooks"))
}

fn install_hook_templates() -> Result<(), String> {
    let hooks_dir = hooks_dir()?;
    fs::create_dir_all(&hooks_dir)
        .map_err(|error| format!("failed to create hooks dir: {error}"))?;

    let claude_template = r#"{
  "hook": "pupkit-claude",
  "transport": "unix_socket",
  "socket": "$HOME/.local/share/pupkit/pupkitd.sock"
}
"#;
    let codex_template = r#"{
  "hook": "pupkit-codex",
  "transport": "unix_socket",
  "socket": "$HOME/.local/share/pupkit/pupkitd.sock"
}
"#;

    fs::write(hooks_dir.join("claude.json"), claude_template)
        .map_err(|error| format!("failed to write claude hook template: {error}"))?;
    fs::write(hooks_dir.join("codex.json"), codex_template)
        .map_err(|error| format!("failed to write codex hook template: {error}"))?;

    println!("installed pupkit hook templates to {}", hooks_dir.display());
    Ok(())
}

fn doctor_hook_templates() -> Result<(), String> {
    let hooks_dir = hooks_dir()?;
    let claude = hooks_dir.join("claude.json");
    let codex = hooks_dir.join("codex.json");
    println!("hooks dir: {}", hooks_dir.display());
    println!(
        "claude template: {}",
        if claude.exists() { "ok" } else { "missing" }
    );
    println!(
        "codex template: {}",
        if codex.exists() { "ok" } else { "missing" }
    );
    Ok(())
}
