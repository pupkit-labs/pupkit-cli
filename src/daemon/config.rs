use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub state_path: PathBuf,
}

impl DaemonConfig {
    pub fn default_for_home(home: Option<PathBuf>) -> Self {
        let home = home.unwrap_or_else(|| PathBuf::from("."));
        Self {
            socket_path: home.join(".local/share/pupkit/pupkitd.sock"),
            state_path: home.join(".local/share/pupkit/daemon-state.json"),
        }
    }
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
    }
}
