#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    Welcome,
    SystemSummary,
    AiTools,
    Services,
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
pub struct PublicIpSummary {
    pub address: String,
    pub country_label: String,
    pub source: PublicIpSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicIpSource {
    Live,
    Cache,
    Disabled,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiToolsSummary {
    pub claude_model: String,
    pub claude_skills: String,
    pub codex_model: String,
    pub codex_skills: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceEntry {
    pub name: String,
    pub manager: ServiceManager,
    pub status: ServiceStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceManager {
    Brew,
    Systemd,
    SysV,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceStatus {
    Running,
    Stopped,
    Error,
    Unknown,
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
