#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    Welcome,
    SystemSummary,
    Help,
    Version,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemSummary {
    pub host: String,
    pub os_label: String,
    pub arch: String,
    pub shell_label: String,
    pub project_stage: String,
}
