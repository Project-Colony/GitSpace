use serde_json::json;

use crate::dotnet::{DotnetClient, DotnetRequest};

fn dotnet_available() -> bool {
    std::process::Command::new("dotnet")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[test]
fn ipc_handshake_ping_ok() {
    if !dotnet_available() {
        eprintln!("Skipping IPC handshake test: dotnet runtime not available.");
        return;
    }

    let request = DotnetRequest {
        id: "req-test-handshake".to_string(),
        command: "ping".to_string(),
        payload: json!({}),
    };

    let response = DotnetClient::helper()
        .send_request(&request)
        .expect("dotnet helper should respond to ping");

    assert_eq!(response.id, request.id);
    assert_eq!(response.status, "ok");
    let payload = response
        .payload
        .expect("ping response should include payload");
    assert!(
        payload.get("version").and_then(|value| value.as_str()).is_some(),
        "ping payload should include version string"
    );
}
