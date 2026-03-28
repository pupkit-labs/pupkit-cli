#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    Welcome,
    SystemSummary,
    AiTools,
    Help,
    Version,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemSummary {
    pub os_label: String,
    pub load_label: String,
    pub host_label: String,
    pub disk_label: String,
    pub cpu_label: String,
    pub shell_label: String,
    pub memory_label: String,
    pub proxy_label: String,
    pub uptime_label: String,
    pub time_label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiToolsSummary {
    pub claude_model: String,
    pub claude_skills: String,
    pub codex_model: String,
    pub codex_skills: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WelcomeSnapshot {
    pub timestamp: String,
    pub user_label: String,
    pub host_label: String,
    pub current_dir: String,
    pub system: SystemSummary,
    pub ai_tools: AiToolsSummary,
}
