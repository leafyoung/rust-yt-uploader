//! YouTube API types and utilities.
//!
//! This module provides the [`YouTubeClient`] (split across domain-focused
//! submodules: upload, video, playlist, caption, comment) plus YouTube API
//! response types and utilities organized in a single location for better
//! maintainability and LLM-friendly code regeneration.

pub mod types;

mod caption;
mod comment;
mod playlist;
mod upload;
mod video;

// Re-export all types
pub use types::*;

// Re-export upload drivers
pub use upload::{upload_batch_concurrent, upload_batch_sequential, upload_individual_sequential};

use anyhow::{Result, anyhow};
use futures::stream::{self, Stream};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use url::Url;

use crate::google_oauth::GoogleOAuth;
use crate::models::RetryConfig;

/// YouTube API service configuration
pub const YOUTUBE_API_SERVICE_NAME: &str = "youtube";
pub const YOUTUBE_API_VERSION: &str = "v3";
pub const YOUTUBE_API_BASE_URL: &str = "https://www.googleapis.com";

/// OAuth 2.0 scopes required for YouTube operations
pub const YOUTUBE_UPLOAD_SCOPE: &str = "https://www.googleapis.com/auth/youtube.upload";
pub const YOUTUBE_SCOPE: &str = "https://www.googleapis.com/auth/youtube";
pub const YOUTUBE_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";
pub const YOUTUBE_PLAYLIST_SCOPE: &str = "https://www.googleapis.com/auth/youtube.force-ssl";

/// Build the YouTube API base URL
pub fn build_youtube_base_url() -> String {
    let mut url = Url::parse(YOUTUBE_API_BASE_URL).expect("Invalid base URL");
    url.path_segments_mut()
        .expect("URL cannot be base")
        .push(YOUTUBE_API_SERVICE_NAME)
        .push(YOUTUBE_API_VERSION);
    url.to_string()
}

/// Build the YouTube direct upload URL
pub fn build_youtube_direct_upload_url() -> String {
    let mut url = Url::parse(YOUTUBE_API_BASE_URL).expect("Invalid base URL");
    url.path_segments_mut()
        .expect("URL cannot be base")
        .push("upload")
        .push(YOUTUBE_API_SERVICE_NAME)
        .push(YOUTUBE_API_VERSION)
        .push("videos");

    url.query_pairs_mut()
        .append_pair("uploadType", "multipart")
        .append_pair("part", "snippet,status,recordingDetails");

    url.to_string()
}

/// Get credentials path for a specific profile
///
/// # Arguments
/// * `profile` - Profile name (required, alphanumeric only)
///
/// # Returns
/// * PathBuf to the credentials file (client_secret-{profile}.json)
///
/// # Errors
/// * Returns error if profile name is empty or contains invalid characters
pub fn credentials_path_for_profile(profile: &str) -> anyhow::Result<std::path::PathBuf> {
    validate_profile_name(profile)?;
    Ok(std::path::PathBuf::from(format!(
        "client_secret-{}.json",
        profile
    )))
}

/// Resolve an existing OAuth client secrets file for a profile.
///
/// The OAuth client secret identifies the Google Cloud Console application,
/// not a particular user. It can be reused across profiles. The token file
/// (`youtube-oauth2-{profile}.json`) is what distinguishes profiles.
///
/// Resolution order:
/// 1. `client_secret-{profile}.json`
/// 2. `client_secret.json`
/// 3. The only `client_secret-*.json` file in the current directory
///
/// When the profile-specific client_secret is missing but another one exists,
/// this function returns the existing one so the OAuth challenge can launch
/// and produce a new `youtube-oauth2-{profile}.json` token.
///
/// # Arguments
/// * `profile` - Profile name (required, alphanumeric only)
///
/// # Returns
/// * PathBuf to an existing client secrets file
///
/// # Errors
/// * Returns error if no client secrets file exists or multiple fallback
///   files make the choice ambiguous.
pub fn resolve_credentials_path_for_profile(profile: &str) -> anyhow::Result<std::path::PathBuf> {
    let profile_path = credentials_path_for_profile(profile)?;
    if profile_path.exists() {
        return Ok(profile_path);
    }

    let default_path = std::path::PathBuf::from("client_secret.json");
    if default_path.exists() {
        return Ok(default_path);
    }

    let mut fallback_paths: Vec<std::path::PathBuf> = std::fs::read_dir(".")?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("client_secret-") && name.ends_with(".json"))
        })
        .collect();
    fallback_paths.sort();

    match fallback_paths.len() {
        0 => anyhow::bail!(
            "No OAuth client secrets file found. Expected {} or client_secret.json. Download one from Google Cloud Console and try again.",
            profile_path.display()
        ),
        1 => Ok(fallback_paths.remove(0)),
        _ => anyhow::bail!(
            "Client secrets file {} not found and multiple {} files exist ({}). Copy or symlink the intended one to {}.",
            profile_path.display(),
            fallback_paths.len(),
            fallback_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            profile_path.display()
        ),
    }
}

/// Validate profile name - only alphanumeric characters allowed
///
/// # Arguments
/// * `profile` - The profile name to validate
///
/// # Returns
/// * `Ok(())` if valid, `Err` with description if invalid
pub fn validate_profile_name(profile: &str) -> anyhow::Result<()> {
    if profile.is_empty() {
        anyhow::bail!("Profile name cannot be empty");
    }
    if !profile.chars().all(|c| c.is_ascii_alphanumeric()) {
        anyhow::bail!(
            "Profile name must contain only alphanumeric characters (a-z, A-Z, 0-9), no spaces or special characters"
        );
    }
    Ok(())
}

/// Get token path for a specific profile
///
/// # Arguments
/// * `profile` - Profile name (required, alphanumeric only)
///
/// # Returns
/// * PathBuf to the token file (youtube-oauth2-{profile}.json)
///
/// # Errors
/// * Returns error if profile name is empty or contains invalid characters
pub fn token_path_for_profile(profile: &str) -> anyhow::Result<std::path::PathBuf> {
    validate_profile_name(profile)?;
    Ok(std::path::PathBuf::from(format!(
        "youtube-oauth2-{}.json",
        profile
    )))
}

/// Default YouTube OAuth scopes
pub fn default_youtube_scopes() -> Vec<&'static str> {
    vec![
        YOUTUBE_UPLOAD_SCOPE,
        YOUTUBE_PLAYLIST_SCOPE,
        YOUTUBE_SCOPE,
        YOUTUBE_READONLY_SCOPE,
    ]
}

/// Progress reporter trait and implementations, public DTOs, and the client core.
/// Progress reporter trait for upload progress tracking
pub trait ProgressReporter: Send + Sync {
    fn report_progress(&self, uploaded: u64, total: u64, filename: &str);
    fn finish(&self);
}

/// No-op progress reporter that does nothing
pub struct NoProgress;

impl ProgressReporter for NoProgress {
    fn report_progress(&self, _uploaded: u64, _total: u64, _filename: &str) {}
    fn finish(&self) {}
}

/// Progress bar implementation using indicatif
pub struct ProgressBarReporter {
    bar: ProgressBar,
}

impl ProgressBarReporter {
    pub fn new(filename: &str, total_size: u64) -> Self {
        let bar = ProgressBar::new(total_size);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta}) {msg}"
            )
            .unwrap()
            .progress_chars("#>-")
        );
        bar.set_message(format!("Uploading {}", filename));

        Self { bar }
    }
}

impl ProgressReporter for ProgressBarReporter {
    fn report_progress(&self, uploaded: u64, _total: u64, _filename: &str) {
        self.bar.set_position(uploaded);
    }

    fn finish(&self) {
        self.bar.finish_with_message("Upload complete");
    }
}

/// Video details for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDetails {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub upload_date: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    pub tags: Vec<String>,
    #[serde(rename = "defaultLanguage")]
    pub default_language: Option<String>,
    #[serde(rename = "defaultAudioLanguage")]
    pub default_audio_language: Option<String>,
    #[serde(rename = "recordingDate")]
    pub recording_date: Option<String>,
    pub duration: Option<String>,
    pub caption: Option<String>,
}

/// Caption/subtitle details for a video
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionDetails {
    pub id: String,
    #[serde(rename = "videoId")]
    pub video_id: String,
    pub language: String,
    #[serde(rename = "isAutoSynced")]
    pub is_auto_synced: Option<bool>,
    #[serde(rename = "isCC")]
    pub is_cc: Option<bool>,
    #[serde(rename = "isLarge")]
    pub is_large: Option<bool>,
    #[serde(rename = "isDraft")]
    pub is_draft: Option<bool>,
    pub name: Option<String>,
    #[serde(rename = "audioTrackType")]
    pub audio_track_type: Option<String>,
    #[serde(rename = "isEasyReader")]
    pub is_easy_reader: Option<bool>,
}

/// YouTube video uploader
pub struct YouTubeClient {
    client: GoogleOAuth,
    retry_config: RetryConfig,
    progress_reporter: Arc<dyn ProgressReporter>,
}

impl YouTubeClient {
    /// Create a new YouTube uploader with a specific profile
    ///
    /// # Arguments
    /// * `profile` - Profile name (alphanumeric only).
    ///   Credentials will be loaded from `client_secret-{profile}.json`
    ///   Token will be saved to/loaded from `youtube-oauth2-{profile}.json`
    ///
    /// # Errors
    /// * Returns error if profile name contains invalid characters
    pub async fn new(profile: &str) -> Result<Self> {
        let credentials_path = resolve_credentials_path_for_profile(profile)?;
        let token_path = token_path_for_profile(profile)?;
        Self::with_credentials_path(credentials_path, token_path).await
    }

    /// Create a new YouTube uploader with a custom credentials path
    pub async fn with_credentials_path<P: AsRef<Path>>(
        credentials_path: P,
        token_path: P,
    ) -> Result<Self> {
        let scopes = default_youtube_scopes();

        let client = GoogleOAuth::new(
            credentials_path,
            token_path,
            scopes,
            build_youtube_base_url(),
        )
        .await?;
        let retry_config = RetryConfig::default();

        Ok(Self {
            client,
            retry_config,
            progress_reporter: Arc::new(NoProgress),
        })
    }

    /// Create a new YouTube uploader with profile and custom progress reporter
    ///
    /// # Arguments
    /// * `profile` - Profile name (alphanumeric only).
    ///   Credentials will be loaded from `client_secret-{profile}.json`
    ///   Token will be saved to/loaded from `youtube-oauth2-{profile}.json`
    /// * `progress_reporter` - Custom progress reporter implementation
    pub async fn with_progress_reporter(
        profile: &str,
        progress_reporter: Arc<dyn ProgressReporter>,
    ) -> Result<Self> {
        let credentials_path = resolve_credentials_path_for_profile(profile)?;
        let token_path = token_path_for_profile(profile)?;
        let scopes = default_youtube_scopes();

        let client = GoogleOAuth::new(
            credentials_path,
            token_path,
            scopes,
            build_youtube_base_url(),
        )
        .await?;
        let retry_config = RetryConfig::default();

        Ok(Self {
            client,
            retry_config,
            progress_reporter,
        })
    }

    /// Execute an HTTP request and parse the JSON response, with standardized error handling.
    ///
    /// This helper eliminates duplicate error handling code across API methods.
    ///
    /// # Arguments
    /// * `request` - The request builder to execute
    /// * `operation` - Description of the operation for error messages
    ///
    /// # Returns
    /// * The deserialized response type T
    async fn execute_and_parse<T>(&self, request: RequestBuilder, operation: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "YouTube API {} failed with status {}: {}",
                operation,
                status,
                text
            ));
        }
        Ok(response.json().await?)
    }

    /// Fetch all videos from the user's channel.
    ///
    /// This method retrieves all videos from the authenticated user's channel
    /// with their details including video ID, title, description, status, dates, etc.
    ///
    /// # Returns
    /// * Result containing a vector of VideoDetails
    ///
    /// # API Endpoint
    /// GET <https://www.googleapis.com/youtube/v3/search?part=snippet&forMine=true&type=video&maxResults=50&pageToken={pageToken}>
    /// GET <https://www.googleapis.com/youtube/v3/videos?part=snippet,status,recordingDetails&id={video_ids}>
    /// Stream all videos from the user's channel page by page (50 per page).
    ///
    /// Each item is one page of full video details. Memory stays O(page)
    /// instead of O(channel), so callers that process videos incrementally
    /// never materialize the whole channel. Fetch errors surface as `Err`
    /// items and terminate the stream.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use anyhow::Result;
    /// # use futures::StreamExt;
    /// # async fn example(client: &rust_yt_uploader::YouTubeClient) -> Result<()> {
    /// let pages = client.video_pages();
    /// futures::pin_mut!(pages);
    /// while let Some(page) = pages.next().await {
    ///     eprintln!("got {} videos", page?.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn video_pages(&self) -> impl Stream<Item = Result<Vec<VideoDetails>>> + '_ {
        stream::unfold(Some(None::<String>), move |state| async move {
            let page_token = state?; // None => stream exhausted
            match self.fetch_video_page(page_token).await {
                Ok((page, next)) if !page.is_empty() => Some((Ok(page), Some(next))),
                Ok(_) => None,                  // empty page = end of channel
                Err(e) => Some((Err(e), None)), // yield the error, then terminate
            }
        })
    }

    /// Fetch one page of channel videos (search + per-video details).
    /// Returns the page and the token for the next page, if any.
    async fn fetch_video_page(
        &self,
        page_token: Option<String>,
    ) -> Result<(Vec<VideoDetails>, Option<String>)> {
        // Build the search endpoint with pagination
        let mut endpoint =
            String::from("search?part=snippet&forMine=true&type=video&maxResults=50");
        if let Some(token) = &page_token {
            endpoint.push_str(&format!("&pageToken={}", token));
        }

        let response = self.client.get(&endpoint).await?.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to list videos with status {}: {}",
                status,
                text
            ));
        }

        let search_response: types::SearchResponse = response.json().await?;

        let video_ids: Vec<String> = search_response
            .items
            .iter()
            .map(|item| item.id.video_id.clone())
            .collect();

        let video_details = self.fetch_video_details(&video_ids).await?;
        Ok((video_details, search_response.next_page_token))
    }

    /// Fetch detailed information for a list of video IDs.
    ///
    /// # Arguments
    /// * `video_ids` - Vector of YouTube video IDs
    ///
    /// # Returns
    /// * Result containing a vector of VideoDetails
    async fn fetch_video_details(&self, video_ids: &[String]) -> Result<Vec<VideoDetails>> {
        if video_ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids_string = video_ids.join(",");
        let endpoint = format!(
            "videos?part=snippet,status,recordingDetails,contentDetails&id={}",
            ids_string
        );

        let video_response: types::VideoResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video details")
            .await?;

        let videos = video_response
            .items
            .into_iter()
            .map(|item| VideoDetails {
                id: item.id,
                title: item.snippet.title,
                description: item.snippet.description,
                status: item.status.privacy_status,
                upload_date: item.snippet.published_at,
                category_id: item.snippet.category_id,
                tags: item.snippet.tags.unwrap_or_default(),
                default_language: item.snippet.default_language,
                default_audio_language: item.snippet.default_audio_language,
                recording_date: item.recording_details.and_then(|rd| rd.recording_date),
                duration: item
                    .content_details
                    .as_ref()
                    .and_then(|cd| cd.duration.clone()),
                caption: item.content_details.and_then(|cd| cd.caption),
            })
            .collect();

        Ok(videos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_profile_name_valid() {
        // Valid alphanumeric names
        assert!(validate_profile_name("abc").is_ok());
        assert!(validate_profile_name("ABC").is_ok());
        assert!(validate_profile_name("123").is_ok());
        assert!(validate_profile_name("abc123").is_ok());
        assert!(validate_profile_name("ABC123").is_ok());
        assert!(validate_profile_name("a1B2c3").is_ok());
        assert!(validate_profile_name("profile1").is_ok());
        assert!(validate_profile_name("MyProfile").is_ok());
    }

    #[test]
    fn test_validate_profile_name_invalid() {
        // Empty string
        assert!(validate_profile_name("").is_err());

        // Contains spaces
        assert!(validate_profile_name("my profile").is_err());

        // Contains special characters
        assert!(validate_profile_name("my-profile").is_err());
        assert!(validate_profile_name("my_profile").is_err());
        assert!(validate_profile_name("my.profile").is_err());
        assert!(validate_profile_name("profile@1").is_err());
        assert!(validate_profile_name("profile#1").is_err());
        assert!(validate_profile_name("profile/1").is_err());
        assert!(validate_profile_name("profile\\1").is_err());

        // Unicode characters
        assert!(validate_profile_name("プロファイル").is_err());
        assert!(validate_profile_name("配置文件").is_err());
    }

    #[test]
    fn test_token_path_for_profile() {
        // Valid profile names
        let path = token_path_for_profile("work").unwrap();
        assert_eq!(path, std::path::PathBuf::from("youtube-oauth2-work.json"));

        let path = token_path_for_profile("Profile123").unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("youtube-oauth2-Profile123.json")
        );

        // Empty profile should fail
        assert!(token_path_for_profile("").is_err());

        // Invalid profile names should fail
        assert!(token_path_for_profile("invalid-profile").is_err());
        assert!(token_path_for_profile("invalid profile").is_err());
    }

    #[test]
    fn test_credentials_path_for_profile() {
        // Valid profile names
        let path = credentials_path_for_profile("work").unwrap();
        assert_eq!(path, std::path::PathBuf::from("client_secret-work.json"));

        let path = credentials_path_for_profile("Profile123").unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("client_secret-Profile123.json")
        );

        // Empty profile should fail
        assert!(credentials_path_for_profile("").is_err());

        // Invalid profile names should fail
        assert!(credentials_path_for_profile("invalid-profile").is_err());
        assert!(credentials_path_for_profile("invalid profile").is_err());
    }
}
