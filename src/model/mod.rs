#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    Welcome,
    SystemSummary,
    AiTools,
    AiUsage,
    Install,
    Services,
    Details,
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
            match country_flag_emoji(country_label) {
                Some(flag) => format!("{country_label} {flag} · {address}"),
                None => format!("{country_label} · {address}"),
            }
        }
    }
}

fn country_flag_emoji(country_label: &str) -> Option<String> {
    let country_code = normalize_country_code(country_label)?;
    let mut emoji = String::new();

    for letter in country_code.chars() {
        let base = 0x1F1E6;
        let offset = u32::from(letter).checked_sub(u32::from('A'))?;
        let codepoint = char::from_u32(base + offset)?;
        emoji.push(codepoint);
    }

    Some(emoji)
}

fn normalize_country_code(country_label: &str) -> Option<String> {
    let trimmed = country_label.trim();
    if trimmed.len() == 2
        && trimmed
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return Some(trimmed.to_ascii_uppercase());
    }

    let normalized = trimmed.to_ascii_lowercase();
    match normalized.as_str() {
        "united states" | "united states of america" => Some("US".to_string()),
        "china" | "people's republic of china" => Some("CN".to_string()),
        "japan" => Some("JP".to_string()),
        "singapore" => Some("SG".to_string()),
        "hong kong" => Some("HK".to_string()),
        "taiwan" => Some("TW".to_string()),
        "south korea" | "korea, republic of" | "republic of korea" => Some("KR".to_string()),
        "united kingdom" | "great britain" | "britain" => Some("GB".to_string()),
        "germany" => Some("DE".to_string()),
        "france" => Some("FR".to_string()),
        "canada" => Some("CA".to_string()),
        "australia" => Some("AU".to_string()),
        "netherlands" => Some("NL".to_string()),
        _ => None,
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
    pub resets_at_epoch_secs: Option<u64>,
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
pub struct CopilotUsageSummary {
    pub availability: UsageAvailability,
    pub model: String,
    pub plan_type: String,
    pub last_active_at: String,
    pub total_requests: Option<u64>,
    pub last_24h_requests: Option<u64>,
    pub total_sessions: Option<u64>,
    pub remaining_percent: Option<u8>,
    pub hint: String,
    pub quota: Option<CopilotQuotaInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopilotQuotaInfo {
    pub login: String,
    pub plan: String,
    pub reset_date: String,
    pub premium: CopilotQuotaEntry,
    pub chat: CopilotQuotaEntry,
    pub completions: CopilotQuotaEntry,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopilotQuotaEntry {
    pub entitlement: u64,
    pub remaining: u64,
    /// Stored as fixed-point (multiplied by 10) to keep Eq/PartialEq derivable.
    /// e.g. 95.6% is stored as 956.
    pub percent_remaining_x10: u64,
    pub unlimited: bool,
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
    pub copilot: CopilotUsageSummary,
}

#[cfg(test)]
mod tests {
    use super::{PublicIpSource, PublicIpSummary};

    #[test]
    fn display_label_appends_flag_for_country_name() {
        let summary = PublicIpSummary {
            address: "149.28.91.67".to_string(),
            country_label: "United States".to_string(),
            source: PublicIpSource::Cache,
        };

        assert_eq!(summary.display_label(), "United States 🇺🇸 · 149.28.91.67");
    }

    #[test]
    fn display_label_appends_flag_for_country_code() {
        let summary = PublicIpSummary {
            address: "149.28.91.67".to_string(),
            country_label: "US".to_string(),
            source: PublicIpSource::Live,
        };

        assert_eq!(summary.display_label(), "US 🇺🇸 · 149.28.91.67");
    }

    #[test]
    fn display_label_skips_flag_when_country_is_unknown() {
        let summary = PublicIpSummary {
            address: "149.28.91.67".to_string(),
            country_label: "Unknown Region".to_string(),
            source: PublicIpSource::Live,
        };

        assert_eq!(summary.display_label(), "Unknown Region · 149.28.91.67");
    }
}
