use anyhow::Result;
use reqwest::{Client, RequestBuilder};
use std::path::Path;

use super::oauth::OAuthFlow;

/// YouTube API client with OAuth 2.0 authentication using yup-oauth2
#[derive(Clone)]
pub struct GoogleOAuth {
    http_client: Client,
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

        Ok(Self {
            http_client: Client::new(),
            access_token: credentials.access_token,
            base_url,
        })
    }

    /// Create an authenticated request builder with Authorization header
    async fn _authenticated_request(
        &self,
        method: reqwest::Method,
        url: &str,
    ) -> Result<RequestBuilder> {
        let mut request = self.http_client.request(method, url);
        request = request.header("Authorization", format!("Bearer {}", self.access_token));

        Ok(request)
    }

    /// Create a GET request to the YouTube API
    pub async fn get(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self._authenticated_request(reqwest::Method::GET, &url)
            .await
    }

    /// Create a POST request to the YouTube API
    pub async fn post(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self._authenticated_request(reqwest::Method::POST, &url)
            .await
    }

    /// Create a PUT request to the YouTube API
    #[allow(unused)]
    pub async fn put(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self._authenticated_request(reqwest::Method::PUT, &url)
            .await
    }

    /// Create a POST request to the YouTube API
    pub async fn delete(&self, endpoint: &str) -> Result<RequestBuilder> {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        self._authenticated_request(reqwest::Method::DELETE, &url)
            .await
    }

    /// Create a generic authenticated request
    pub async fn request(&self, method: reqwest::Method, url: &str) -> Result<RequestBuilder> {
        self._authenticated_request(method, url).await
    }
}
