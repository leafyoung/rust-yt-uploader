//! YouTube API types and utilities.
//!
//! This module provides YouTube API response types organized in a single location
//! for better maintainability and LLM-friendly code regeneration.

pub mod types;

// Re-export all types
pub use types::*;

// Re-export utility functions
use url::Url;

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

/// Default path to OAuth credentials file
pub fn default_credentials_path() -> std::path::PathBuf {
    std::path::PathBuf::from("client_secret.json")
}

/// Default path to OAuth token file
pub fn default_token_path() -> std::path::PathBuf {
    std::path::PathBuf::from("youtube-oauth2.json")
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
