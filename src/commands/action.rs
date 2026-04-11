use crate::daemon::{DaemonConfig, client::send_request};
use crate::protocol::{ClientRequest, ServerResponse, UiAction};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionCommand {
    Approve {
        request_id: String,
        always: bool,
    },
    Deny {
        request_id: String,
    },
    AnswerOption {
        request_id: String,
        option_id: String,
    },
    AnswerText {
        request_id: String,
        text: String,
    },
}

pub fn execute(command: ActionCommand) -> Result<(), String> {
    let ui_action = match command {
        ActionCommand::Approve { request_id, always } => UiAction::Approve {
            request_id: crate::protocol::RequestId::new(request_id),
            always,
        },
        ActionCommand::Deny { request_id } => UiAction::Deny {
            request_id: crate::protocol::RequestId::new(request_id),
        },
        ActionCommand::AnswerOption {
            request_id,
            option_id,
        } => UiAction::AnswerOption {
            request_id: crate::protocol::RequestId::new(request_id),
            option_id,
        },
        ActionCommand::AnswerText { request_id, text } => UiAction::AnswerText {
            request_id: crate::protocol::RequestId::new(request_id),
            text,
        },
    };

    let config = DaemonConfig::default_for_home(std::env::var_os("HOME").map(Into::into));
    let response = send_request(config.socket_path.as_path(), &ClientRequest::Ui(ui_action))?;
    let output = serde_json::to_string_pretty(&response)
        .map_err(|error| format!("failed to serialize action response: {error}"))?;
    println!("{output}");

    match response {
        ServerResponse::Error { message } => Err(message),
        _ => Ok(()),
    }
}
