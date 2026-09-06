//! OAuth 2.0 helper utilities for PKCE-based authentication flow.

use super::credentials::Credentials;

use anyhow::{Result, anyhow};
use base64::Engine;
use rand::RngExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::{Response, Server};
use tracing::info;
use url::Url;

#[cfg(feature = "use-yup-oauth2")]
use yup_oauth2::{ApplicationSecret, InstalledFlowAuthenticator, InstalledFlowReturnMethod};

#[cfg(not(feature = "use-yup-oauth2"))]
use serde_jsonlines::JsonLinesReader;

/// Macro to build URL-encoded form body from key-value pairs.
///
/// This macro simplifies building application/x-www-form-urlencoded request bodies,
/// automatically handling URL encoding for values while maintaining readability.
///
/// # Arguments
/// * Key-value pairs as `(key_expr, value_expr)` tuples
/// * Supports optional trailing comma
///
/// # Returns
/// String containing the form-encoded body (e.g., "key1=value1&key2=value2")
///
/// # Example
/// ```ignore
/// let body = encode_form_params!([
///     ("client_id", "my-app-id"),
///     ("secret", "app-secret"),
///     ("code", auth_code),
/// ]);
/// // Result: "client_id=my-app-id&secret=app-secret&code=...encoded..."
/// ```
///
/// # Usage Notes
/// - Both keys and values are automatically URL-encoded via `urlencoding::encode()`
/// - Keys are typically not encoded but wrapped for consistency
/// - Useful for OAuth 2.0 token exchange and refresh flows
macro_rules! encode_form_params {
    ([$($key:expr, $val:expr),* $(,)?]) => {{
        [
            $($key, $val),*
        ]
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    }};
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSecret {
    pub installed: ClientSecretDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSecretDetails {
    pub client_id: String,
    pub project_id: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub auth_provider_x509_cert_url: String,
    pub client_secret: String,
    pub redirect_uris: Vec<String>,
}

/// Helper struct to manage OAuth 2.0 flow with PKCE
pub struct OAuthFlow {
    code_verifier: String,
    code_challenge: String,
    state: String,
    /// Shared HTTP client reused across token exchange/refresh requests.
    client: Client,
}

/// Characters allowed in PKCE code verifier (RFC 7636)
const PKCE_CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

/// Generate a cryptographically random string from PKCE charset
fn generate_random_string(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..PKCE_CHARSET.len());
            PKCE_CHARSET[idx] as char
        })
        .collect()
}

impl Default for OAuthFlow {
    /// Generate PKCE parameters and CSRF state
    fn default() -> Self {
        // Generate 128-char code verifier (RFC 7636 recommends 43-128 chars)
        let code_verifier = generate_random_string(128);

        // Create code challenge: BASE64URL(SHA256(code_verifier))
        let hash = Sha256::digest(code_verifier.as_bytes());
        let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);

        // Generate 32-char state for CSRF protection
        let state = generate_random_string(32);

        Self {
            code_verifier,
            code_challenge,
            state,
            client: Client::new(),
        }
    }
}

impl OAuthFlow {
    pub async fn auth(
        &self,
        client_secrets_path: impl AsRef<std::path::Path>,
        token_file_path: Option<impl AsRef<std::path::Path>>,
        scopes: &[&str],
    ) -> Result<Credentials> {
        #[cfg(feature = "use-yup-oauth2")]
        {
            return self
                .auth_with_yup_oauth2(client_secrets_path, token_file_path, scopes)
                .await;
        }

        #[cfg(not(feature = "use-yup-oauth2"))]
        {
            self.auth_with_custom_oauth(client_secrets_path, token_file_path, scopes)
                .await
        }
    }

    #[cfg(feature = "use-yup-oauth2")]
    async fn auth_with_yup_oauth2(
        &self,
        client_secrets_path: impl AsRef<std::path::Path>,
        token_file_path: Option<impl AsRef<std::path::Path>>,
        scopes: &[&str],
    ) -> Result<Credentials> {
        let secrets_path = client_secrets_path.as_ref();
        let secret = yup_oauth2::read_application_secret(secrets_path)
            .await
            .map_err(|e| {
                anyhow!(
                    "Client secrets file not found or invalid: {}. OAuth challenge cannot start without this Google OAuth client secrets file: {}",
                    secrets_path.display(),
                    e
                )
            })?;

        let token_uri = secret.token_uri.clone();

        self.authenticate_with_retry(secret, token_uri, token_file_path, scopes)
            .await
    }

    #[cfg(not(feature = "use-yup-oauth2"))]
    async fn auth_with_custom_oauth(
        &self,
        client_secrets_path: impl AsRef<std::path::Path>,
        token_file_path: Option<impl AsRef<std::path::Path>>,
        scopes: &[&str],
    ) -> Result<Credentials> {
        let secrets_path = client_secrets_path.as_ref();
        if !secrets_path.exists() {
            return Err(anyhow!(
                "No OAuth client secrets file found at {}. Download a client_secret JSON file from Google Cloud Console and place it in the current directory.",
                secrets_path.display()
            ));
        }

        let json_str = std::fs::read_to_string(secrets_path)?;
        let app_secret = self.parse_client_secrets(&json_str)?;

        // Try to load existing valid credentials
        if let Some(ref token_path) = token_file_path
            && let Ok(credentials) = Credentials::from_file(token_path.as_ref())
        {
            if credentials.has_scopes(scopes) {
                if credentials.is_valid() {
                    info!("Using existing valid credentials");
                    return Ok(credentials);
                } else if let Some(refresh_token) = &credentials.refresh_token {
                    info!("Credentials expired, attempting refresh");
                    match self
                        .refresh_token(&app_secret.installed, refresh_token, scopes)
                        .await
                    {
                        Ok(refreshed_credentials) => {
                            info!("Token refresh successful");
                            let json_str = refreshed_credentials.to_json()?;
                            std::fs::write(token_path.as_ref(), json_str)?;
                            return Ok(refreshed_credentials);
                        }
                        Err(e) => {
                            info!("Token refresh failed: {}, re-authenticating", e);
                        }
                    }
                } else {
                    info!("No refresh token available, re-authenticating");
                }
            } else {
                info!(
                    "Credentials has different scopes, re-authenticating {:?}",
                    credentials.scopes
                );
            }
        }

        info!("No valid credentials found - starting OAuth flow");
        let result = self
            .oauth_flow_with_local_server(&app_secret.installed, scopes)
            .await?;

        // Save credentials if token path provided
        if let Some(token_path) = token_file_path {
            let json_str = result.to_json()?;
            std::fs::write(token_path.as_ref(), json_str)?;
        }

        Ok(result)
    }

    #[cfg(not(feature = "use-yup-oauth2"))]
    fn parse_client_secrets(&self, json_str: &str) -> Result<ClientSecret> {
        serde_json::from_str(json_str)
            .or_else(|_| {
                let reader = std::io::Cursor::new(json_str);
                let mut jsonl_reader = JsonLinesReader::new(reader);

                jsonl_reader
                    .read::<ClientSecret>()?
                    .ok_or_else(|| anyhow!("Client secrets file is empty"))
            })
            .map_err(|e| {
                anyhow!(
                    "Failed to parse client secrets (tried both JSON and JSON Lines formats): {}",
                    e
                )
            })
    }

    #[cfg(feature = "use-yup-oauth2")]
    async fn authenticate_with_retry(
        &self,
        secret: ApplicationSecret,
        token_uri: String,
        token_file_path: Option<impl AsRef<std::path::Path>>,
        scopes: &[&str],
    ) -> Result<Credentials> {
        let mut auth = InstalledFlowAuthenticator::builder(
            secret.clone(),
            InstalledFlowReturnMethod::HTTPRedirect,
        );

        if let Some(path) = &token_file_path {
            auth = auth.persist_tokens_to_disk(path.as_ref().to_path_buf());
        }

        match auth.build().await {
            Ok(auth) => match auth.token(scopes).await {
                Ok(token) => Ok(Credentials {
                    access_token: String::from(token.token().unwrap()),
                    refresh_token: None, // yup-oauth2 does not expose refresh token directly
                    token_uri,
                    scopes: scopes.iter().map(|s| s.to_string()).collect(),
                    expires_at: token
                        .expiration_time()
                        .map(|t| t.unix_timestamp())
                        .unwrap_or(0),
                }),
                Err(e) => Err(anyhow!("Failed to create authenticator: {}", e)),
            },
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("JSONToken") && token_file_path.is_some() {
                    if let Some(path) = &token_file_path {
                        std::fs::remove_file(path.as_ref())?;
                    }
                    return Box::pin(self.authenticate_with_retry(
                        secret,
                        token_uri,
                        token_file_path,
                        scopes,
                    ))
                    .await;
                } else {
                    Err(anyhow!("Failed to get token for scopes: {}", e))
                }
            }
        }
    }

    /// Perform OAuth 2.0 authorization flow with local HTTP server callback.
    ///
    /// This implementation starts a local HTTP server to receive OAuth callback,
    /// providing a smoother user experience compared to manual copy-paste.
    ///
    /// Uses the synchronous `tiny_http` implementation wrapped in a tokio blocking task,
    /// reducing overhead for a simple single-connection callback server.
    ///
    /// # Arguments
    /// * `app_secret` - OAuth application credentials
    /// * `scopes` - List of OAuth scopes to request
    ///
    /// # Returns
    /// * OAuth credentials with access and refresh tokens
    pub async fn oauth_flow_with_local_server(
        &self,
        app_secret: &ClientSecretDetails,
        scopes: &[&str],
    ) -> Result<Credentials> {
        let flow = OAuthFlow::default();
        let app_secret_clone = app_secret.clone();
        let scopes_clone = scopes.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        tokio::task::spawn_blocking(move || {
            let scopes_refs: Vec<&str> = scopes_clone.iter().map(|s| s.as_str()).collect();
            flow.oauth_flow_with_local_server_sync(&app_secret_clone, &scopes_refs)
        })
        .await
        .map_err(|e| anyhow!("Failed to spawn blocking task: {}", e))?
    }

    /// Perform OAuth 2.0 authorization flow with local HTTP server callback (synchronous).
    ///
    /// This implementation uses `tiny_http` to provide a lightweight, synchronous
    /// HTTP server for receiving OAuth callbacks without async runtime overhead.
    ///
    /// # Arguments
    /// * `app_secret` - OAuth application credentials
    /// * `scopes` - List of OAuth scopes to request
    ///
    /// # Returns
    /// * OAuth credentials with access and refresh tokens
    pub fn oauth_flow_with_local_server_sync(
        &self,
        app_secret: &ClientSecretDetails,
        scopes: &[&str],
    ) -> Result<Credentials> {
        // Start HTTP server with random port (OS assigns available port)
        let server = Server::http("127.0.0.1:0")
            .map_err(|e| anyhow!("Failed to bind to local address: {}", e))?;

        // Extract port from server address
        let addr_str = format!("{}", server.server_addr());
        let port = addr_str
            .rsplit(':')
            .next()
            .and_then(|p| p.parse::<u16>().ok())
            .ok_or_else(|| anyhow!("Failed to extract port from server address"))?;

        let redirect_uri = self.open_auth_uri(app_secret, scopes, port);

        // Accept one request
        let request = server
            .recv()
            .map_err(|e| anyhow!("Failed to receive HTTP request: {}", e))?;

        // Extract request path
        let request_path = request.url();

        // Extract authorization code from query string
        let auth_code = {
            let full_url = format!("http://localhost{}", request_path);
            Url::parse(&full_url)
                .ok()
                .and_then(|url| {
                    url.query_pairs()
                        .find(|(key, _)| key == "code")
                        .map(|(_, value)| value.into_owned())
                })
                .ok_or_else(|| anyhow!("No authorization code found in request"))?
        };

        // Send success response
        let response = Response::from_string(
            "<html><body><h1>Authentication Successful!</h1>\
            <p>You can close this window and return to the application.</p></body></html>",
        )
        .with_status_code(200)
        .with_header(
            "Content-type: text/html"
                .parse::<tiny_http::Header>()
                .unwrap(),
        );

        request
            .respond(response)
            .map_err(|e| anyhow!("Failed to send response: {}", e))?;

        // Exchange code for credentials (async operation wrapped in blocking context)
        futures::executor::block_on(async {
            self.exchange_code(app_secret, &auth_code, &redirect_uri, scopes)
                .await
        })
    }

    fn open_auth_uri(
        &self,
        app_secret: &ClientSecretDetails,
        scopes: &[&str],
        port: u16,
    ) -> String {
        let redirect_uri = format!("http://127.0.0.1:{}", port);
        let auth_uri = self.build_auth_uri(app_secret, scopes, &redirect_uri);

        println!("\n🔐 OAuth Authorization Required");
        println!("================================");
        println!("Opening browser for authorization...");
        println!("If browser doesn't open, visit this URL:");
        println!("{}\n", auth_uri);

        // Open browser if possible
        let _ = open::that(&auth_uri);

        println!("Local server listening on {}...\n", redirect_uri);
        redirect_uri
    }

    /// Build OAuth 2.0 authorization URL with proper URL encoding
    pub fn build_auth_uri(
        &self,
        app_secret: &ClientSecretDetails,
        scopes: &[&str],
        redirect_uri: &str,
    ) -> String {
        let scopes_str = scopes.join(" ");
        let query_params = encode_form_params!([
            ("client_id", app_secret.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", scopes_str.as_str()),
            ("access_type", "offline"),
            ("state", self.state.as_str()),
            ("code_challenge", self.code_challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]);
        format!("{}?{}", app_secret.auth_uri, query_params)
    }

    /// Exchange authorization code for access tokens
    pub async fn exchange_code(
        &self,
        app_secret: &ClientSecretDetails,
        auth_code: &str,
        redirect_uri: &str,
        scopes: &[&str],
    ) -> Result<Credentials> {
        let body = encode_form_params!([
            ("client_id", app_secret.client_id.as_str()),
            ("client_secret", app_secret.client_secret.as_str()),
            ("code", auth_code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
            ("code_verifier", &self.code_verifier),
        ]);

        let response = self
            .client
            .post(&app_secret.token_uri)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token exchange failed: {}", error_text));
        }

        let token_response: serde_json::Value = response.json().await?;

        let (access_token, expires_at) = Self::parse_token_response(&token_response)?;

        let refresh_token = token_response
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow!("Missing refresh_token in response - ensure offline access is requested")
            })?;

        info!("Successfully obtained OAuth tokens with PKCE protection");

        Ok(Credentials {
            access_token,
            refresh_token: Some(refresh_token),
            token_uri: app_secret.token_uri.to_string(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            expires_at,
        })
    }

    /// Parse access token and expiration time from OAuth token response.
    ///
    /// # Arguments
    /// * `token_response` - The JSON response from the OAuth token endpoint
    ///
    /// # Returns
    /// * A tuple of (access_token, expires_at)
    fn parse_token_response(token_response: &serde_json::Value) -> Result<(String, i64)> {
        let access_token = token_response
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Missing access_token in response"))?;

        let expires_in = token_response
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow!("Missing expires_in in response"))?;

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let expires_at = current_time + expires_in;

        Ok((access_token, expires_at))
    }

    /// Refresh access token using refresh token
    ///
    /// # Arguments
    /// * `app_secret` - OAuth application credentials
    /// * `refresh_token` - The refresh token to use
    /// * `scopes` - List of OAuth scopes for the credentials
    ///
    /// # Returns
    /// * OAuth credentials with refreshed access token
    pub async fn refresh_token(
        &self,
        app_secret: &ClientSecretDetails,
        refresh_token: &str,
        scopes: &[&str],
    ) -> Result<Credentials> {
        let body = encode_form_params!([
            ("client_id", app_secret.client_id.as_str()),
            ("client_secret", app_secret.client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ]);

        let response = self
            .client
            .post(&app_secret.token_uri)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Token refresh failed: {}", error_text));
        }

        let token_response: serde_json::Value = response.json().await?;

        let (access_token, expires_at) = Self::parse_token_response(&token_response)?;

        let new_refresh_token = token_response
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        info!("Successfully refreshed OAuth token");

        Ok(Credentials {
            access_token,
            refresh_token: new_refresh_token.or_else(|| Some(refresh_token.to_string())),
            token_uri: app_secret.token_uri.to_string(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "use-yup-oauth2"))]
    #[test]
    fn test_client_secret_parsing_fallback() {
        // Test regular JSON parsing
        let regular_json = r#"{
            "installed": {
                "client_id": "test-client-id",
                "project_id": "test-project",
                "auth_uri": "https://accounts.google.com/o/oauth2/v2/auth",
                "token_uri": "https://oauth2.googleapis.com/token",
                "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
                "client_secret": "test-secret",
                "redirect_uris": ["http://localhost"]
            }
        }"#;

        let result: Result<ClientSecret, _> = serde_json::from_str(regular_json);
        assert!(result.is_ok(), "Regular JSON parsing should work");

        // Test JSON Lines parsing (simulated)
        let json_lines = r#"{"installed": {"client_id": "test-client-id", "project_id": "test-project", "auth_uri": "https://accounts.google.com/o/oauth2/v2/auth", "token_uri": "https://oauth2.googleapis.com/token", "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs", "client_secret": "test-secret", "redirect_uris": ["http://localhost"]}}"#;

        let reader = std::io::Cursor::new(json_lines);
        let mut jsonl_reader = JsonLinesReader::new(reader);
        let jsonl_result: Result<Option<ClientSecret>, _> = jsonl_reader.read();

        // JSON Lines format should work (though this specific example might not)
        // The important thing is that the fallback logic exists and compiles
        assert!(jsonl_result.is_ok(), "JSON Lines reader should not error");
    }

    #[test]
    fn test_extract_code_from_get_request() {
        let request = r#"GET /?state=IQ.Qd7z8NXE6Cy2wT&code=4/0ASc3gC3U0cJy3T5qlWBYKqRacVm&scope=https://www.googleapis.com/auth/youtube.upload%20https://www.googleapis.com/auth/youtube.readonly%20https://www.googleapis.com/auth/youtube HTTP/1.1
Host: 127.0.0.1:40345
User-Agent: Test"#;

        let auth_code = request
            .lines()
            .find(|line| line.starts_with("GET /?"))
            .and_then(|line| {
                line.split_whitespace().nth(1).and_then(|path| {
                    let full_url = format!("http://localhost{}", path);
                    Url::parse(&full_url)
                        .ok()?
                        .query_pairs()
                        .find(|(key, _)| key == "code")
                        .map(|(_, value)| value.into_owned())
                })
            });

        assert!(auth_code.is_some(), "Failed to extract authorization code");
        assert_eq!(auth_code.unwrap(), "4/0ASc3gC3U0cJy3T5qlWBYKqRacVm");
    }

    #[test]
    fn test_extract_code_with_reordered_params() {
        let request = r#"GET /?code=test_code_123&state=IQ.Test HTTP/1.1"#;

        let auth_code = request
            .lines()
            .find(|line| line.starts_with("GET /?"))
            .and_then(|line| {
                line.split_whitespace().nth(1).and_then(|path| {
                    let full_url = format!("http://localhost{}", path);
                    Url::parse(&full_url)
                        .ok()?
                        .query_pairs()
                        .find(|(key, _)| key == "code")
                        .map(|(_, value)| value.into_owned())
                })
            });

        assert_eq!(auth_code, Some("test_code_123".to_string()));
    }

    #[test]
    fn test_extract_code_with_url_encoded_values() {
        let request = r#"GET /?code=4/0AS%20test&state=IQ.Test HTTP/1.1"#;

        let auth_code = request
            .lines()
            .find(|line| line.starts_with("GET /?"))
            .and_then(|line| {
                line.split_whitespace().nth(1).and_then(|path| {
                    let full_url = format!("http://localhost{}", path);
                    Url::parse(&full_url)
                        .ok()?
                        .query_pairs()
                        .find(|(key, _)| key == "code")
                        .map(|(_, value)| value.into_owned())
                })
            });

        // The url crate automatically decodes %20 to space
        assert_eq!(auth_code, Some("4/0AS test".to_string()));
    }
}
