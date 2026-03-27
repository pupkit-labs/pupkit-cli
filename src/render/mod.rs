use crate::model::SystemSummary;

pub fn render_welcome(summary: &SystemSummary) -> String {
    format!(
        "\
PUP CLI START RUST
==================
welcome prototype bootstrap

host   : {host}
os     : {os} ({arch})
shell  : {shell}
stage  : {stage}
",
        host = summary.host,
        os = summary.os_label,
        arch = summary.arch,
        shell = summary.shell_label,
        stage = summary.project_stage,
    )
}

pub fn render_system_summary(summary: &SystemSummary) -> String {
    format!(
        "\
host={host}
os={os}
arch={arch}
shell={shell}
stage={stage}
",
        host = summary.host,
        os = summary.os_label,
        arch = summary.arch,
        shell = summary.shell_label,
        stage = summary.project_stage,
    )
}

#[cfg(test)]
mod tests {
    use crate::model::SystemSummary;

    use super::{render_system_summary, render_welcome};

    #[test]
    fn welcome_render_includes_core_fields() {
        let summary = SystemSummary {
            host: "liupx-host".to_string(),
            os_label: "macOS".to_string(),
            arch: "arm64".to_string(),
            shell_label: "zsh 5.9".to_string(),
            project_stage: "bootstrap skeleton ready".to_string(),
        };

        let output = render_welcome(&summary);
        assert!(output.contains("PUP CLI START RUST"));
        assert!(output.contains("liupx-host"));
        assert!(output.contains("bootstrap skeleton ready"));
    }

    #[test]
    fn system_summary_render_uses_key_value_shape() {
        let summary = SystemSummary {
            host: "liupx-host".to_string(),
            os_label: "macOS".to_string(),
            arch: "arm64".to_string(),
            shell_label: "zsh 5.9".to_string(),
            project_stage: "bootstrap skeleton ready".to_string(),
        };

        let output = render_system_summary(&summary);
        assert!(output.contains("host=liupx-host"));
        assert!(output.contains("shell=zsh 5.9"));
    }
}
