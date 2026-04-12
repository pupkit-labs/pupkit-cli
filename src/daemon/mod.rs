mod app;
pub mod client;
mod config;
mod pending;
mod persistence;
mod priority;
mod registry;
mod server;
pub mod shell_launcher;
pub mod tty_inject;
pub mod watcher;

pub use app::PupkitDaemon;
pub use config::DaemonConfig;
pub use priority::collect_attention_sessions;
pub use priority::select_top_session;
pub use registry::SessionRegistry;
pub use server::DaemonServer;
