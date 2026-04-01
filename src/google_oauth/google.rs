use anyhow::Result;
use reqwest::{Client, RequestBuilder};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::oauth::OAuthFlow;

/// Connection pool settings for optimal throughput
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const POOL_MAX_IDLE: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// YouTube API client with OAuth 2.0 authentication using yup-oauth2
#[derive(Clone)]
pub struct GoogleOAuth {
    http_client: Arc<Client>,
    access_token: String,
    base_url: String,
}

impl GoogleOAuth {
    /// Create a new YouTube client with OAuth 2.0 authentication.
    ///
    /// Implements the InstalledAppFlow pattern from Python:
    /// 1. Checks for existing token file (youtube-oauth2.json)
    /// 2. Validates token (scope, expiration)
    /// 3. Refreshes token if expired
    /// 4. Performs interactive OAuth flow if needed
    /// 5. Persists token to file for future use
    ///
    /// # Arguments
    /// * `client_secrets_path` - Path to client_secret.json file
    /// * `scopes` - List of OAuth scopes to request
    ///
    /// # Returns
    /// * Authenticated YouTube client
    pub async fn new<P: AsRef<Path>>(
        client_secrets_path: P,
        token_file_path: P,
        scopes: Vec<&str>,
        base_url: String,
    ) -> Result<Self> {
        let oauth = OAuthFlow::default();
        let credentials = oauth
            .auth(client_secrets_path, Some(token_file_path), &scopes)
            .await?;

        // Create optimized HTTP client with connection pooling
        let http_client = Arc::new(
            Client::builder()
                .pool_idle_timeout(POOL_IDLE_TIMEOUT)
                .pool_max_idle_per_host(POOL_MAX_IDLE)
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .tcp_keepalive(Duration::from_secs(60))
                .tcp_nodelay(true)
                .build()?,
        );

        Ok(Self {
            http_client,
            access_token: credentials.access_token,
            base_url,
        })
    }

    /// Create an authenticated request builder with Authorization header
    fn authenticated_request(&self, method: reqwest::Method, url: &str) -> Result<RequestBuilder> {
        let mut request = self.http_client.request(method, url);
        request = request.header("Authorization", format!("Bearer {}", self.access_token));

        Ok(request)
    }

    /// Create a GET request to the YouTube API
    pub async fn get(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self.authenticated_request(reqwest::Method::GET, &url)
    }

    /// Create a POST request to the YouTube API
    pub async fn post(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self.authenticated_request(reqwest::Method::POST, &url)
    }

    /// Create a PUT request to the YouTube API
    pub async fn put(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self.authenticated_request(reqwest::Method::PUT, &url)
    }

    /// Create a DELETE request to the YouTube API
    pub async fn delete(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self.authenticated_request(reqwest::Method::DELETE, &url)
    }

    /// Create a generic authenticated request
    pub async fn request(&self, method: reqwest::Method, url: &str) -> Result<RequestBuilder> {
        self.authenticated_request(method, url)
    }

    /// Get a clone of the underlying HTTP client for advanced use cases
    pub fn http_client(&self) -> Arc<Client> {
        self.http_client.clone()
    }
}
