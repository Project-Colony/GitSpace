use serde_json::json;

use crate::dotnet::{DotnetClient, DotnetError, DotnetRequest};
use crate::error::AppError;

fn dotnet_available() -> bool {
    std::process::Command::new("dotnet")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn skip_if_dotnet_unavailable(test_name: &str) -> bool {
    if !dotnet_available() {
        eprintln!("Skipping {test_name}: dotnet runtime not available.");
        return true;
    }

    false
}

#[test]
fn ipc_handshake_ping_ok() {
    if skip_if_dotnet_unavailable("IPC handshake test") {
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

#[test]
fn dotnet_error_mapping_validation() {
    let error = DotnetError {
        category: "InvalidRequest".to_string(),
        message: "Missing payload.kind".to_string(),
        details: None,
    };

    let mapped = super::map_dotnet_error(&error);
    assert!(matches!(mapped, AppError::Validation(_)));
}

#[test]
fn ipc_credential_request_statuses() {
    if skip_if_dotnet_unavailable("credential.request test") {
        return;
    }

    let client = DotnetClient::helper();

    let get_response = client
        .credential_request(crate::dotnet::CredentialRequest {
            service: "gitspace-test".to_string(),
            account: Some("user@example.com".to_string()),
            action: "get".to_string(),
        })
        .expect("credential request should succeed");
    assert!(
        matches!(get_response.status.as_str(), "not_found" | "error"),
        "expected not_found or error when credentials are unavailable, got {}",
        get_response.status
    );
    assert!(get_response.username.is_none());
    assert!(get_response.secret.is_none());

    let store_response = client
        .credential_request(crate::dotnet::CredentialRequest {
            service: "gitspace-test".to_string(),
            account: Some("user@example.com".to_string()),
            action: "store".to_string(),
        })
        .expect("credential store should succeed");
    assert!(
        matches!(store_response.status.as_str(), "denied" | "error"),
        "expected denied or error when credentials cannot be stored, got {}",
        store_response.status
    );
    assert!(store_response.username.is_none());
    assert!(store_response.secret.is_none());
}

#[test]
fn ipc_library_call() {
    if skip_if_dotnet_unavailable("library.call test") {
        return;
    }

    let client = DotnetClient::helper();

    let response = client
        .library_call(crate::dotnet::LibraryCallRequest {
            name: "system.info".to_string(),
            payload: json!({}),
        })
        .expect("library call should succeed");
    assert!(response.payload.get("os").is_some());
    assert!(response.payload.get("version").is_some());

    let error = client.library_call(crate::dotnet::LibraryCallRequest {
        name: "unknown.library".to_string(),
        payload: json!({}),
    });
    assert!(matches!(error, Err(AppError::Validation(_))));
}
