use std::io::{self, Read};

use serde_json::Value;

use crate::adapters::claude::normalize_claude_event;
use crate::adapters::codex::normalize_codex_event;
use crate::daemon::{DaemonConfig, client::send_request};
use crate::protocol::{ClientRequest, HookEnvelope, ServerResponse, SessionEventKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeSource {
    Claude,
    Codex,
}

pub fn execute(source: BridgeSource) -> Result<(), String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| format!("failed to read bridge input: {error}"))?;
    let value: Value = serde_json::from_str(&input)
        .map_err(|error| format!("failed to parse bridge JSON: {error}"))?;

    let event = match source {
        BridgeSource::Claude => normalize_claude_event(&value)?,
        BridgeSource::Codex => normalize_codex_event(&value)?,
    };

    let expects_response = matches!(
        event.kind,
        SessionEventKind::ApprovalRequested | SessionEventKind::QuestionRequested
    );
    let request = ClientRequest::Hook(HookEnvelope {
        event,
        expects_response,
    });

    let config = DaemonConfig::default_for_home(std::env::var_os("HOME").map(Into::into));
    let response = send_request(config.socket_path.as_path(), &request)?;
    let output = serde_json::to_string_pretty(&response)
        .map_err(|error| format!("failed to serialize bridge response: {error}"))?;
    println!("{output}");

    match response {
        ServerResponse::Error { message } => Err(message),
        _ => Ok(()),
    }
}
