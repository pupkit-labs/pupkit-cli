pub mod ai_tools;
pub mod ai_usage;
pub mod copilot;
pub mod system;

use crate::model::WelcomeSnapshot;

use self::ai_tools::collect_ai_tools_summary;
use self::ai_usage::collect_ai_usage_summary;
use self::copilot::{collect_copilot_usage_summary_fast, finish_copilot_usage_summary};
use self::system::{
    collect_public_ip_summary, collect_system_summary_fast, detect_time_label, detect_user_label,
};

pub fn collect_fast_snapshot() -> WelcomeSnapshot {
    WelcomeSnapshot {
        timestamp: detect_time_label(),
        user_label: detect_user_label(),
        system: collect_system_summary_fast(),
        ai_tools: collect_ai_tools_summary(),
        ai_usage: collect_ai_usage_summary(),
        copilot: collect_copilot_usage_summary_fast(),
    }
}

pub fn collect_welcome_snapshot() -> WelcomeSnapshot {
    let mut snapshot = collect_fast_snapshot();
    snapshot.system.public_ip = collect_public_ip_summary();
    snapshot.copilot = finish_copilot_usage_summary(snapshot.copilot);
    snapshot
}
