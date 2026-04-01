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
