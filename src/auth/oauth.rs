//! OAuth2 sign-in for a desktop app, which cannot keep a client secret.
//!
//! GitHub uses the device flow ([`DeviceFlow`], RFC 8628): its token endpoint requires a client
//! secret for the authorization code flow, even with PKCE. GitLab uses the authorization code flow
//! with PKCE ([`OAuthFlow`]) and a callback on a loopback port.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, DeviceAuthorizationUrl,
    DeviceCodeErrorResponse, DeviceCodeErrorResponseType, EndpointNotSet, EndpointSet, HttpRequest,
    HttpResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    StandardDeviceAuthorizationResponse, TokenResponse, TokenUrl,
};
use reqwest::blocking::Client;
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// OAuth callback server timeout.
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(120);

/// Local callback port. GitLab only accepts the exact redirect URI registered with the
/// application, so the port cannot change from one sign-in to the next.
const CALLBACK_PORT: u16 = 9876;

/// Timeout of each OAuth HTTP request.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Grant type of a device flow token request (RFC 8628 section 3.4).
const DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// How much longer to wait between polls after a `slow_down` answer (RFC 8628 section 3.5).
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);

/// Longest wait between polls while the token endpoint cannot be reached.
const MAX_POLL_BACKOFF: Duration = Duration::from_secs(60);

const CODE_EXPIRED: &str = "The code expired before it was approved. Start the sign-in again.";

/// OAuth provider configuration.
#[derive(Debug, Clone)]
pub struct OAuthProvider {
    pub name: String,
    pub client_id: String,
    pub auth_url: String,
    /// Device authorization endpoint, for providers signed in with [`DeviceFlow`].
    pub device_auth_url: Option<String>,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub host: String,
}

impl OAuthProvider {
    /// Creates GitHub OAuth configuration, signed in with [`DeviceFlow`].
    ///
    /// Note: You need to register a GitHub OAuth App at
    /// https://github.com/settings/developers with "Enable Device Flow" ticked.
    /// The callback URL is not used.
    pub fn github(client_id: &str) -> Self {
        Self {
            name: "GitHub".to_string(),
            client_id: client_id.to_string(),
            auth_url: "https://github.com/login/oauth/authorize".to_string(),
            device_auth_url: Some("https://github.com/login/device/code".to_string()),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            scopes: vec!["repo".to_string(), "read:user".to_string()],
            host: "github.com".to_string(),
        }
    }

    /// Creates GitLab OAuth configuration, signed in with [`OAuthFlow`].
    ///
    /// Note: You need to register a GitLab OAuth Application at
    /// https://gitlab.com/-/user_settings/applications with "Confidential" unticked.
    ///
    /// Set the callback URL to: http://127.0.0.1:9876/callback
    pub fn gitlab(client_id: &str) -> Self {
        Self {
            name: "GitLab".to_string(),
            client_id: client_id.to_string(),
            auth_url: "https://gitlab.com/oauth/authorize".to_string(),
            device_auth_url: None,
            token_url: "https://gitlab.com/oauth/token".to_string(),
            scopes: vec![
                "read_user".to_string(),
                "read_repository".to_string(),
                "write_repository".to_string(),
            ],
            host: "gitlab.com".to_string(),
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
    #[allow(dead_code)] // token refresh is not wired yet
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

/// Authorization code flow with PKCE and a loopback callback (GitLab).
pub struct OAuthFlow {
    provider: OAuthProvider,
    client: BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>,
    pkce_verifier: Option<PkceCodeVerifier>,
    csrf_token: Option<CsrfToken>,
}

impl OAuthFlow {
    /// Creates a new OAuth flow for the given provider.
    pub fn new(provider: OAuthProvider) -> Result<Self, String> {
        // Checked before the browser opens, so a busy port is reported up front.
        TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).map_err(|e| {
            format!(
                "Port {CALLBACK_PORT} is needed for the GitLab sign-in callback but is in use ({e}). \
                 Close the program using it and try again."
            )
        })?;
        let redirect_url = format!("http://127.0.0.1:{CALLBACK_PORT}/callback");

        // No client secret: PKCE public client.
        let client = BasicClient::new(ClientId::new(provider.client_id.clone()))
            .set_auth_uri(
                AuthUrl::new(provider.auth_url.clone())
                    .map_err(|e| format!("Invalid auth URL: {}", e))?,
            )
            .set_token_uri(
                TokenUrl::new(provider.token_url.clone())
                    .map_err(|e| format!("Invalid token URL: {}", e))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_url)
                    .map_err(|e| format!("Invalid redirect URL: {}", e))?,
            );

        Ok(Self {
            provider,
            client,
            pkce_verifier: None,
            csrf_token: None,
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
        let listener = TcpListener::bind(("127.0.0.1", CALLBACK_PORT))
            .map_err(|e| format!("Failed to bind callback server: {}", e))?;

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
            port = CALLBACK_PORT,
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
                    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

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

        let http_client = match http_client() {
            Ok(client) => client,
            Err(err) => return OAuthResult::Error(err),
        };

        let token_result = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request(&|request| send(&http_client, request));

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

/// Sign-in with the OAuth device flow (GitHub).
///
/// [`DeviceFlow::start`] gets a short user code; the user enters it at
/// [`DeviceFlow::verification_uri`] and [`DeviceFlow::wait`] polls the token endpoint until they
/// approve or deny it, or the code expires. Only the client ID is needed, no client secret and no
/// local callback server.
pub struct DeviceFlow {
    provider: OAuthProvider,
    http_client: Client,
    details: StandardDeviceAuthorizationResponse,
}

impl DeviceFlow {
    /// Asks the provider for a user code. Blocks on one HTTP request.
    pub fn start(provider: OAuthProvider) -> Result<Self, String> {
        let device_auth_url = provider
            .device_auth_url
            .clone()
            .ok_or_else(|| format!("{} does not support the device flow", provider.name))?;
        let client = BasicClient::new(ClientId::new(provider.client_id.clone()))
            .set_device_authorization_url(
                DeviceAuthorizationUrl::new(device_auth_url)
                    .map_err(|e| format!("Invalid device authorization URL: {e}"))?,
            );
        let http_client = http_client()?;

        info!(
            target: "gitspace::auth::oauth",
            provider = %provider.name,
            "Requesting a device code"
        );
        let details: StandardDeviceAuthorizationResponse = client
            .exchange_device_code()
            .add_scopes(provider.scopes.iter().cloned().map(Scope::new))
            .request(&|request| send(&http_client, request))
            .map_err(|e| format!("Could not get a sign-in code from {}: {e}", provider.name))?;

        // The user is sent to this page, so never to anything but HTTPS.
        if details.verification_uri().url().scheme() != "https" {
            return Err(format!(
                "{} sent a verification page that is not HTTPS",
                provider.name
            ));
        }

        Ok(Self {
            provider,
            http_client,
            details,
        })
    }

    /// The code the user enters on the verification page.
    pub fn user_code(&self) -> &str {
        self.details.user_code().secret()
    }

    /// The page where the user enters the code, always `https://`.
    pub fn verification_uri(&self) -> &str {
        self.details.verification_uri().url().as_str()
    }

    /// Polls the token endpoint until the user approves or denies the code or it expires.
    ///
    /// Returns [`OAuthResult::Cancelled`] as soon as `cancel` receives a message or its sender is
    /// dropped.
    pub fn wait(self, cancel: &Receiver<()>) -> OAuthResult {
        let deadline = Instant::now() + self.details.expires_in();
        let mut interval = self.details.interval();

        loop {
            if !matches!(
                cancel.recv_timeout(interval),
                Err(RecvTimeoutError::Timeout)
            ) {
                info!(target: "gitspace::auth::oauth", "Device flow cancelled");
                return OAuthResult::Cancelled;
            }
            if Instant::now() >= deadline {
                return OAuthResult::Error(CODE_EXPIRED.to_string());
            }

            let poll = match self.request_token() {
                Ok((status, body)) => next_device_poll(status, &body, interval, &self.provider),
                Err(err) => {
                    warn!(
                        target: "gitspace::auth::oauth",
                        provider = %self.provider.name,
                        error = %err,
                        "Token endpoint unreachable, retrying"
                    );
                    DevicePoll::Retry(backoff(interval))
                }
            };
            match poll {
                DevicePoll::Retry(next) => interval = next,
                DevicePoll::Done(result) => {
                    if matches!(result, OAuthResult::Success(_)) {
                        info!(
                            target: "gitspace::auth::oauth",
                            provider = %self.provider.name,
                            "Device flow completed successfully"
                        );
                    }
                    return result;
                }
            }
        }
    }

    /// Sends one token request and returns the HTTP status and body.
    fn request_token(&self) -> Result<(u16, Vec<u8>), reqwest::Error> {
        let response = self
            .http_client
            .post(&self.provider.token_url)
            .header(ACCEPT, "application/json")
            .form(&[
                ("client_id", self.provider.client_id.as_str()),
                ("device_code", self.details.device_code().secret()),
                ("grant_type", DEVICE_CODE_GRANT),
            ])
            .send()?;
        let status = response.status().as_u16();
        Ok((status, response.bytes()?.to_vec()))
    }
}

/// What to do after one answer of the token endpoint during the device flow.
#[derive(Debug)]
enum DevicePoll {
    /// Not approved yet: poll again after this interval.
    Retry(Duration),
    /// Approved, denied, expired or failed: stop polling.
    Done(OAuthResult),
}

/// Reads one answer of the token endpoint during the device flow.
fn next_device_poll(
    status: u16,
    body: &[u8],
    interval: Duration,
    provider: &OAuthProvider,
) -> DevicePoll {
    // GitHub answers 200 OK to pending and failed polls too, so the body decides, not the status.
    if let Ok(response) = serde_json::from_slice::<DeviceCodeErrorResponse>(body) {
        let result = match response.error() {
            DeviceCodeErrorResponseType::AuthorizationPending => {
                return DevicePoll::Retry(interval)
            }
            DeviceCodeErrorResponseType::SlowDown => {
                return DevicePoll::Retry(interval + SLOW_DOWN_STEP)
            }
            DeviceCodeErrorResponseType::ExpiredToken => CODE_EXPIRED.to_string(),
            DeviceCodeErrorResponseType::AccessDenied => {
                format!("The sign-in was denied on {}.", provider.name)
            }
            _ => format!("{} refused the sign-in: {response}", provider.name),
        };
        return DevicePoll::Done(OAuthResult::Error(result));
    }

    match serde_json::from_slice::<BasicTokenResponse>(body) {
        Ok(response) => DevicePoll::Done(OAuthResult::Success(OAuthToken::from_response(
            &response, provider,
        ))),
        // A rate limit or a server or proxy error page is passing: ask again later.
        Err(_) if status == 429 || status >= 500 => DevicePoll::Retry(backoff(interval)),
        Err(_) => DevicePoll::Done(OAuthResult::Error(format!(
            "Unexpected answer from {} (HTTP {status})",
            provider.name
        ))),
    }
}

/// Doubles the poll interval, up to [`MAX_POLL_BACKOFF`], while the token endpoint cannot answer
/// (RFC 8628 section 3.5).
fn backoff(interval: Duration) -> Duration {
    interval.max((interval * 2).min(MAX_POLL_BACKOFF))
}

/// Builds the HTTP client for OAuth requests.
///
/// Redirects stay off, as oauth2 recommends, so an endpoint cannot bounce a request (and the code
/// or token in it) elsewhere.
fn http_client() -> Result<Client, String> {
    Client::builder()
        .user_agent("gitspace")
        .timeout(HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))
}

/// Sends an oauth2 request through `client`, the app's reqwest client with TLS.
fn send(client: &Client, request: HttpRequest) -> Result<HttpResponse, io::Error> {
    let response = client
        .execute(request.try_into().map_err(io::Error::other)?)
        .map_err(io::Error::other)?;
    let mut builder = oauth2::http::Response::builder().status(response.status());
    for (name, value) in response.headers() {
        builder = builder.header(name, value);
    }
    let body = response.bytes().map_err(io::Error::other)?.to_vec();
    builder.body(body).map_err(io::Error::other)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_provider_config() {
        let provider = OAuthProvider::github("test_client_id");
        assert_eq!(provider.name, "GitHub");
        assert_eq!(provider.host, "github.com");
        assert!(provider.scopes.contains(&"repo".to_string()));
        assert!(provider.device_auth_url.is_some());
    }

    #[test]
    fn gitlab_provider_config() {
        let provider = OAuthProvider::gitlab("test_client_id");
        assert_eq!(provider.name, "GitLab");
        assert_eq!(provider.host, "gitlab.com");
        assert!(provider.scopes.contains(&"read_repository".to_string()));
        assert!(provider.device_auth_url.is_none());
    }

    const INTERVAL: Duration = Duration::from_secs(5);

    fn poll(status: u16, body: &str) -> DevicePoll {
        next_device_poll(
            status,
            body.as_bytes(),
            INTERVAL,
            &OAuthProvider::github("id"),
        )
    }

    #[test]
    fn device_poll_keeps_the_interval_while_pending() {
        // GitHub answers 200, RFC 8628 servers 400: both mean "ask again".
        for status in [200, 400] {
            let body = r#"{"error":"authorization_pending","error_description":"waiting"}"#;
            assert!(matches!(poll(status, body), DevicePoll::Retry(i) if i == INTERVAL));
        }
    }

    #[test]
    fn device_poll_slows_down_by_five_seconds() {
        let body = r#"{"error":"slow_down","interval":10}"#;
        assert!(matches!(
            poll(200, body),
            DevicePoll::Retry(i) if i == INTERVAL + Duration::from_secs(5)
        ));
    }

    #[test]
    fn device_poll_stops_when_the_code_expired_or_was_denied() {
        let expired = poll(200, r#"{"error":"expired_token"}"#);
        assert!(matches!(expired, DevicePoll::Done(OAuthResult::Error(e)) if e == CODE_EXPIRED));

        let denied = poll(200, r#"{"error":"access_denied"}"#);
        assert!(matches!(denied, DevicePoll::Done(OAuthResult::Error(e)) if e.contains("denied")));

        let other = poll(200, r#"{"error":"device_flow_disabled"}"#);
        assert!(matches!(
            other,
            DevicePoll::Done(OAuthResult::Error(e)) if e.contains("device_flow_disabled")
        ));
    }

    #[test]
    fn device_poll_returns_the_token_on_approval() {
        // GitHub's shape: lowercase token type, comma-separated scopes, no expiry.
        let body = r#"{"access_token":"gho_abc","token_type":"bearer","scope":"repo,read:user"}"#;
        match poll(200, body) {
            DevicePoll::Done(OAuthResult::Success(token)) => {
                assert_eq!(token.access_token, "gho_abc");
                assert_eq!(token.host, "github.com");
                assert_eq!(token.provider, "GitHub");
                assert_eq!(token.expires_at, None);
            }
            other => panic!("expected a token, got {other:?}"),
        }
    }

    #[test]
    fn device_poll_reports_an_unexpected_answer() {
        let result = poll(404, "<html>Not found</html>");
        assert!(matches!(result, DevicePoll::Done(OAuthResult::Error(e)) if e.contains("404")));
    }

    #[test]
    fn device_poll_backs_off_on_a_rate_limit_or_server_error() {
        for status in [429, 502, 503] {
            let result = poll(status, "<html>Bad gateway</html>");
            assert!(matches!(result, DevicePoll::Retry(i) if i == INTERVAL * 2));
        }
        assert_eq!(backoff(Duration::from_secs(40)), MAX_POLL_BACKOFF);
    }

    #[test]
    fn device_flow_wait_stops_when_cancelled() {
        let details = serde_json::from_str(
            r#"{"device_code":"dc","user_code":"ABCD-1234",
                "verification_uri":"https://github.com/login/device",
                "expires_in":900,"interval":5}"#,
        )
        .unwrap();
        let flow = DeviceFlow {
            provider: OAuthProvider::github("id"),
            http_client: http_client().unwrap(),
            details,
        };
        assert_eq!(flow.user_code(), "ABCD-1234");
        assert_eq!(flow.verification_uri(), "https://github.com/login/device");

        // Dropping the sender is how the UI cancels: no request is sent.
        let (cancel, cancelled) = std::sync::mpsc::channel::<()>();
        drop(cancel);
        assert!(matches!(flow.wait(&cancelled), OAuthResult::Cancelled));
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
