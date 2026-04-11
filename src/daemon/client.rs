use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use crate::protocol::{ClientRequest, ServerResponse};

pub fn send_request(socket_path: &Path, request: &ClientRequest) -> Result<ServerResponse, String> {
    let mut stream = UnixStream::connect(socket_path).map_err(|error| {
        format!(
            "failed to connect to daemon socket {}: {error}",
            socket_path.display()
        )
    })?;
    let payload = serde_json::to_vec(request)
        .map_err(|error| format!("failed to serialize daemon request: {error}"))?;
    stream
        .write_all(&payload)
        .map_err(|error| format!("failed to write daemon request: {error}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| format!("failed to close daemon write half: {error}"))?;

    let mut response_body = String::new();
    stream
        .read_to_string(&mut response_body)
        .map_err(|error| format!("failed to read daemon response: {error}"))?;
    serde_json::from_str(&response_body)
        .map_err(|error| format!("failed to parse daemon response: {error}"))
}
