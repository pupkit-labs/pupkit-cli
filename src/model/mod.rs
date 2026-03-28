#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    Welcome,
    SystemSummary,
    AiTools,
    AiUsage,
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
    pub public_ip: PublicIpSummary,
    pub proxy_label: String,
    pub uptime_label: String,
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

impl PublicIpSummary {
    pub fn display_label(&self) -> String {
        let address = self.address.trim();
        if address.is_empty() || address == "-" {
            return "-".to_string();
        }

        let country_label = self.country_label.trim();
        if country_label.is_empty() {
            address.to_string()
        } else {
            format!("{country_label} · {address}")
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiToolsSummary {
    pub claude_model: String,
    pub claude_skills: String,
    pub codex_model: String,
    pub codex_skills: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiUsageSummary {
    pub claude: ClaudeUsageSummary,
    pub codex: CodexUsageSummary,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeUsageSummary {
    pub availability: UsageAvailability,
    pub source_label: String,
    pub last_active_at: String,
    pub last_24h: TokenBreakdown,
    pub last_7d: TokenBreakdown,
    pub lifetime: TokenBreakdown,
    pub hint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexUsageSummary {
    pub availability: UsageAvailability,
    pub plan_type: String,
    pub last_active_at: String,
    pub last_session_total_tokens: Option<u64>,
    pub model_context_window: Option<u64>,
    pub last_24h_total_tokens: Option<u64>,
    pub last_7d_total_tokens: Option<u64>,
    pub primary_rate_limit: RateLimitWindow,
    pub secondary_rate_limit: RateLimitWindow,
    pub hint: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenBreakdown {
    pub total_tokens: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RateLimitWindow {
    pub label: &'static str,
    pub used_percent: Option<u8>,
    pub window_minutes: Option<u64>,
    pub resets_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UsageAvailability {
    Live,
    Partial,
    Unavailable,
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
    pub ai_usage: AiUsageSummary,
}
