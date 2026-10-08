//! OAuth2 authentication with PKCE for desktop applications.
//!
//! Implements the Authorization Code Flow with PKCE for secure authentication
//! without requiring a client secret (suitable for public/desktop clients).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::reqwest::http_client;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// OAuth callback server timeout.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);

/// Local callback port range to try.
const CALLBACK_PORT_START: u16 = 9876;
const CALLBACK_PORT_END: u16 = 9886;

/// OAuth provider configuration.
#[derive(Debug, Clone)]
pub struct OAuthProvider {
    pub name: String,
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub host: String,
}

impl OAuthProvider {
    /// Creates GitHub OAuth configuration.
    ///
    /// Note: You need to register a GitHub OAuth App at:
    /// https://github.com/settings/developers
    ///
    /// Set the callback URL to: http://127.0.0.1:9876/callback
    pub fn github(client_id: &str) -> Self {
        Self {
            name: "GitHub".to_string(),
            client_id: client_id.to_string(),
            auth_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            scopes: vec!["repo".to_string(), "read:user".to_string()],
            host: "github.com".to_string(),
        }
    }

    /// Creates GitLab OAuth configuration.
    ///
    /// Note: You need to register a GitLab OAuth Application at:
    /// https://gitlab.com/-/profile/applications
    ///
    /// Set the callback URL to: http://127.0.0.1:9876/callback
    pub fn gitlab(client_id: &str) -> Self {
        Self {
            name: "GitLab".to_string(),
            client_id: client_id.to_string(),
            auth_url: "https://gitlab.com/oauth/authorize".to_string(),
            token_url: "https://gitlab.com/oauth/token".to_string(),
            scopes: vec![
                "read_user".to_string(),
                "read_repository".to_string(),
                "write_repository".to_string(),
            ],
            host: "gitlab.com".to_string(),
        }
    }

    /// Creates GitLab OAuth configuration for a self-hosted instance.
    #[allow(dead_code)]
    pub fn gitlab_self_hosted(client_id: &str, base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        Self {
            name: "GitLab".to_string(),
            client_id: client_id.to_string(),
            auth_url: format!("{}/oauth/authorize", base),
            token_url: format!("{}/oauth/token", base),
            scopes: vec![
                "read_user".to_string(),
                "read_repository".to_string(),
                "write_repository".to_string(),
            ],
            host: extract_host_from_url(base_url).unwrap_or_else(|| base_url.to_string()),
        }
    }
}

/// OAuth token with optional refresh token and expiration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    pub provider: String,
    pub host: String,
}

impl OAuthToken {
    /// Creates a new OAuth token from a token response.
    pub fn from_response(response: &BasicTokenResponse, provider: &OAuthProvider) -> Self {
        let expires_at = response.expires_in().map(|duration| {
            let now = chrono::Utc::now().timestamp();
            now + duration.as_secs() as i64
        });

        Self {
            access_token: response.access_token().secret().to_string(),
            refresh_token: response.refresh_token().map(|t| t.secret().to_string()),
            expires_at,
            provider: provider.name.clone(),
            host: provider.host.clone(),
        }
    }

    /// Checks if the token is expired or about to expire (within 5 minutes).
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = chrono::Utc::now().timestamp();
            // Consider expired if less than 5 minutes remaining
            expires_at - now < 300
        } else {
            false
        }
    }
}

/// Result of an OAuth flow.
#[derive(Debug, Clone)]
pub enum OAuthResult {
    Success(OAuthToken),
    Cancelled,
    Error(String),
}

/// OAuth flow state.
pub struct OAuthFlow {
    provider: OAuthProvider,
    client: BasicClient,
    pkce_verifier: Option<PkceCodeVerifier>,
    csrf_token: Option<CsrfToken>,
    redirect_port: u16,
}

impl OAuthFlow {
    /// Creates a new OAuth flow for the given provider.
    pub fn new(provider: OAuthProvider) -> Result<Self, String> {
        let redirect_port = find_available_port()?;
        let redirect_url = format!("http://127.0.0.1:{}/callback", redirect_port);

        let client = BasicClient::new(
            ClientId::new(provider.client_id.clone()),
            None, // No client secret for PKCE public clients
            AuthUrl::new(provider.auth_url.clone())
                .map_err(|e| format!("Invalid auth URL: {}", e))?,
            Some(
                TokenUrl::new(provider.token_url.clone())
                    .map_err(|e| format!("Invalid token URL: {}", e))?,
            ),
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_url).map_err(|e| format!("Invalid redirect URL: {}", e))?,
        );

        Ok(Self {
            provider,
            client,
            pkce_verifier: None,
            csrf_token: None,
            redirect_port,
        })
    }

    /// Gets the authorization URL to open in the browser.
    pub fn get_auth_url(&mut self) -> String {
        // Generate PKCE challenge
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization request
        let mut auth_request = self
            .client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        // Add scopes
        for scope in &self.provider.scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        let (auth_url, csrf_token) = auth_request.url();

        self.pkce_verifier = Some(pkce_verifier);
        self.csrf_token = Some(csrf_token);

        auth_url.to_string()
    }

    /// Starts the OAuth flow by opening the browser and waiting for callback.
    ///
    /// Returns the OAuth token on success.
    pub fn start(mut self) -> OAuthResult {
        let auth_url = self.get_auth_url();

        info!(
            target: "gitspace::auth::oauth",
            provider = %self.provider.name,
            "Starting OAuth flow"
        );

        // Open browser
        if let Err(err) = webbrowser::open(&auth_url) {
            return OAuthResult::Error(format!("Failed to open browser: {}", err));
        }

        // Wait for callback
        match self.wait_for_callback() {
            Ok(code) => self.exchange_code(code),
            Err(err) => OAuthResult::Error(err),
        }
    }

    /// Waits for the OAuth callback on a local HTTP server.
    fn wait_for_callback(&self) -> Result<String, String> {
        let addr = format!("127.0.0.1:{}", self.redirect_port);
        let listener =
            TcpListener::bind(&addr).map_err(|e| format!("Failed to bind callback server: {}", e))?;

        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        let start = Instant::now();
        let expected_state = self
            .csrf_token
            .as_ref()
            .map(|t| t.secret().clone())
            .unwrap_or_default();

        info!(
            target: "gitspace::auth::oauth",
            port = self.redirect_port,
            "Waiting for OAuth callback"
        );

        loop {
            // Check timeout
            if start.elapsed() > CALLBACK_TIMEOUT {
                return Err("OAuth callback timed out".to_string());
            }

            // Try to accept connection
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0; 4096];
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .ok();

                    if let Ok(size) = stream.read(&mut buffer) {
                        let request = String::from_utf8_lossy(&buffer[..size]);

                        // Parse the callback
                        if let Some(result) = parse_callback(&request, &expected_state) {
                            // Send response to browser
                            let response = match &result {
                                Ok(_) => create_success_response(&self.provider.name),
                                Err(err) => create_error_response(err),
                            };
                            let _ = stream.write_all(response.as_bytes());
                            let _ = stream.flush();

                            return result;
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection yet, sleep briefly
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(format!("Failed to accept connection: {}", e));
                }
            }
        }
    }

    /// Exchanges the authorization code for tokens.
    fn exchange_code(self, code: String) -> OAuthResult {
        let Some(pkce_verifier) = self.pkce_verifier else {
            return OAuthResult::Error("PKCE verifier not initialized".to_string());
        };

        info!(
            target: "gitspace::auth::oauth",
            provider = %self.provider.name,
            "Exchanging authorization code for tokens"
        );

        let token_result = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request(http_client);

        match token_result {
            Ok(response) => {
                let token = OAuthToken::from_response(&response, &self.provider);
                info!(
                    target: "gitspace::auth::oauth",
                    provider = %self.provider.name,
                    has_refresh = token.refresh_token.is_some(),
                    "OAuth flow completed successfully"
                );
                OAuthResult::Success(token)
            }
            Err(err) => {
                error!(
                    target: "gitspace::auth::oauth",
                    provider = %self.provider.name,
                    error = %err,
                    "Token exchange failed"
                );
                OAuthResult::Error(format!("Token exchange failed: {}", err))
            }
        }
    }
}

/// Finds an available port for the callback server.
fn find_available_port() -> Result<u16, String> {
    for port in CALLBACK_PORT_START..=CALLBACK_PORT_END {
        if TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok() {
            return Ok(port);
        }
    }
    Err("No available port for OAuth callback".to_string())
}

/// Parses the OAuth callback request.
fn parse_callback(request: &str, expected_state: &str) -> Option<Result<String, String>> {
    // Parse GET request
    let first_line = request.lines().next()?;
    if !first_line.starts_with("GET /callback") {
        return None;
    }

    // Extract query parameters
    let path = first_line.split_whitespace().nth(1)?;
    let query_start = path.find('?')?;
    let query = &path[query_start + 1..];

    let params: HashMap<_, _> = query
        .split('&')
        .filter_map(|param| {
            let mut parts = param.splitn(2, '=');
            Some((parts.next()?, parts.next().unwrap_or("")))
        })
        .collect();

    // Check for error
    if let Some(error) = params.get("error") {
        let description = params.get("error_description").unwrap_or(&"Unknown error");
        return Some(Err(format!("{}: {}", error, description)));
    }

    // Verify state (CSRF protection)
    let state = params.get("state").unwrap_or(&"");
    if *state != expected_state {
        warn!(
            target: "gitspace::auth::oauth",
            "CSRF state mismatch in OAuth callback"
        );
        return Some(Err("Security error: state mismatch".to_string()));
    }

    // Extract authorization code
    let code = params.get("code")?;
    Some(Ok((*code).to_string()))
}

/// Creates an HTML success response for the browser.
fn create_success_response(provider: &str) -> String {
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>GitSpace - Authentication Successful</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
               display: flex; justify-content: center; align-items: center; height: 100vh;
               margin: 0; background: linear-gradient(135deg, #1a1b26 0%, #24283b 100%); color: #c0caf5; }}
        .container {{ text-align: center; padding: 2rem; background: #1a1b26; border-radius: 12px;
                      box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3); }}
        h1 {{ color: #7aa2f7; margin-bottom: 1rem; }}
        p {{ color: #a9b1d6; }}
        .icon {{ font-size: 4rem; margin-bottom: 1rem; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="icon">&#10003;</div>
        <h1>Authentication Successful!</h1>
        <p>You have successfully logged in with {}.</p>
        <p>You can close this window and return to GitSpace.</p>
    </div>
</body>
</html>"#,
        provider
    );

    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    )
}

/// Creates an HTML error response for the browser.
fn create_error_response(error: &str) -> String {
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>GitSpace - Authentication Failed</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
               display: flex; justify-content: center; align-items: center; height: 100vh;
               margin: 0; background: linear-gradient(135deg, #1a1b26 0%, #24283b 100%); color: #c0caf5; }}
        .container {{ text-align: center; padding: 2rem; background: #1a1b26; border-radius: 12px;
                      box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3); }}
        h1 {{ color: #f7768e; margin-bottom: 1rem; }}
        p {{ color: #a9b1d6; }}
        .error {{ background: #292e42; padding: 1rem; border-radius: 8px; margin-top: 1rem;
                  font-family: monospace; color: #f7768e; }}
        .icon {{ font-size: 4rem; margin-bottom: 1rem; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="icon">&#10007;</div>
        <h1>Authentication Failed</h1>
        <p>There was a problem logging in.</p>
        <div class="error">{}</div>
        <p>Please try again in GitSpace.</p>
    </div>
</body>
</html>"#,
        error
    );

    format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html.len(),
        html
    )
}

/// Extracts the host from a URL.
#[allow(dead_code)]
fn extract_host_from_url(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| h.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_provider_config() {
        let provider = OAuthProvider::github("test_client_id");
        assert_eq!(provider.name, "GitHub");
        assert_eq!(provider.host, "github.com");
        assert!(provider.scopes.contains(&"repo".to_string()));
    }

    #[test]
    fn gitlab_provider_config() {
        let provider = OAuthProvider::gitlab("test_client_id");
        assert_eq!(provider.name, "GitLab");
        assert_eq!(provider.host, "gitlab.com");
        assert!(provider.scopes.contains(&"read_repository".to_string()));
    }

    #[test]
    fn parse_callback_extracts_code() {
        let request = "GET /callback?code=abc123&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let result = parse_callback(request, "xyz");
        assert_eq!(result, Some(Ok("abc123".to_string())));
    }

    #[test]
    fn parse_callback_detects_error() {
        let request =
            "GET /callback?error=access_denied&error_description=User+denied&state=xyz HTTP/1.1\r\n";
        let result = parse_callback(request, "xyz");
        assert!(matches!(result, Some(Err(_))));
    }

    #[test]
    fn parse_callback_rejects_wrong_state() {
        let request = "GET /callback?code=abc123&state=wrong HTTP/1.1\r\n";
        let result = parse_callback(request, "expected");
        assert!(matches!(result, Some(Err(_))));
    }

    #[test]
    fn token_expiration_check() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Some(chrono::Utc::now().timestamp() + 60), // 1 minute
            provider: "Test".to_string(),
            host: "test.com".to_string(),
        };
        assert!(token.is_expired()); // Less than 5 minutes = expired

        let token2 = OAuthToken {
            expires_at: Some(chrono::Utc::now().timestamp() + 600), // 10 minutes
            ..token.clone()
        };
        assert!(!token2.is_expired());
    }
}
