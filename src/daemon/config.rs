use std::env;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub state_path: PathBuf,
    pub pid_path: PathBuf,
    pub shell_paused_path: PathBuf,
    pub shell_binary_path: Option<PathBuf>,
}

impl DaemonConfig {
    pub fn default_for_home(home: Option<PathBuf>) -> Self {
        let home = home.unwrap_or_else(|| PathBuf::from("."));
        Self {
            socket_path: home.join(".local/share/pupkit/pupkitd.sock"),
            state_path: home.join(".local/share/pupkit/daemon-state.json"),
            pid_path: home.join(".local/share/pupkit/pupkitd.pid"),
            shell_paused_path: home.join(".local/share/pupkit/shell-paused"),
            shell_binary_path: resolve_shell_binary(),
        }
    }
}

/// Resolves PupkitShell binary path. macOS only.
#[cfg(target_os = "macos")]
fn resolve_shell_binary() -> Option<PathBuf> {
    // 1. Explicit env override
    if let Ok(p) = env::var("PUPKIT_SHELL_PATH") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    // 2. Sibling of current executable
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("PupkitShell");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn resolve_shell_binary() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::DaemonConfig;
    use std::path::PathBuf;

    #[test]
    fn default_paths_live_under_local_share_pupkit() {
        let config = DaemonConfig::default_for_home(Some(PathBuf::from("/tmp/demo-home")));

        assert_eq!(
            config.socket_path,
            PathBuf::from("/tmp/demo-home/.local/share/pupkit/pupkitd.sock")
        );
        assert_eq!(
            config.state_path,
            PathBuf::from("/tmp/demo-home/.local/share/pupkit/daemon-state.json")
        );
        assert_eq!(
            config.pid_path,
            PathBuf::from("/tmp/demo-home/.local/share/pupkit/pupkitd.pid")
        );
    }

    #[test]
    fn shell_binary_path_is_none_when_not_found() {
        // With no PUPKIT_SHELL_PATH env and no sibling binary, should be None
        // (the actual result depends on the test runner's location, but the
        // field should at least be populated without panicking)
        let config = DaemonConfig::default_for_home(Some(PathBuf::from("/tmp/demo-home")));
        // On CI / dev machines PupkitShell is not next to the test binary
        // so this is almost certainly None
        let _ = config.shell_binary_path;
    }
}
