//! Integration tests for the Rust YouTube uploader.
//!
//! These tests verify the complete functionality of the uploader,
//! including configuration parsing, validation, and API interactions.

use anyhow::Result;
use rust_yt_uploader::{
    BatchConfigRoot, CommonConfig, IndividualConfigRoot, PrivacyStatus, VideoCategory,
};
use validator::Validate;

mod common;
use common::create_test_video_file;

#[test]
fn test_individual_config_parsing() -> Result<()> {
    let temp_file = create_test_video_file();
    let file_path = temp_file.path().to_string_lossy().to_string();

    let yaml_content = format!(
        r#"
videos:
  - title: "Test Video 1"
    description: "This is a test video"
    keywords: "test,video,rust"
    file: "{}"
    category: Comedy
    privacyStatus: "private"
    playlistId: "PL1234567890123456"
    defaultAudioLanguage: "en"
    defaultLanguage: "en"
    recordingDate: "2026-01-24"
  - title: "Test Video 2"
    description: "Another test video"
    keywords: "test,video,rust"
    file: "{}"
    category: ScienceTechnology
    privacyStatus: "unlisted"
    playlistId: "PL1234567890123456"
    defaultAudioLanguage: "en"
    defaultLanguage: "en"
    recordingDate: "2026-01-25"
"#,
        file_path, file_path
    );

    let config: IndividualConfigRoot = serde_yaml_ng::from_str(&yaml_content)?;

    assert_eq!(config.videos.len(), 2);
    assert_eq!(config.videos[0].title, "Test Video 1");
    assert_eq!(config.videos[0].category, VideoCategory::Comedy);
    assert_eq!(config.videos[0].privacy_status, PrivacyStatus::Private);
    assert_eq!(config.videos[1].title, "Test Video 2");
    assert_eq!(config.videos[1].category, VideoCategory::ScienceTechnology);
    assert_eq!(config.videos[1].privacy_status, PrivacyStatus::Unlisted);

    Ok(())
}

#[test]
fn test_batch_config_parsing() -> Result<()> {
    let temp_file = create_test_video_file();
    let file_path = temp_file.path().to_string_lossy().to_string();

    let yaml_content = format!(
        r#"
common:
  prefix: "My Series - "
  keywords: "rust,programming,tutorial"
  category: HowtoStyle
  privacyStatus: "private"
  playlistId: "PL1234567890123456"
  defaultAudioLanguage: "en"
  defaultLanguage: "en"
  recordingDate: "2026-01-24"

titles:
  - "Episode 1: Introduction"
  - "Episode 2: Getting Started"

files:
  - "{}"
  - "{}"
"#,
        file_path, file_path
    );

    let config: BatchConfigRoot = serde_yaml_ng::from_str(&yaml_content)?;

    assert_eq!(config.common.prefix, "My Series - ");
    assert_eq!(config.common.keywords, "rust,programming,tutorial");
    assert_eq!(config.common.category, VideoCategory::HowtoStyle);
    assert_eq!(config.common.privacy_status, PrivacyStatus::Private);
    assert_eq!(config.titles.len(), 2);
    assert_eq!(config.files.len(), 2);
    assert_eq!(config.titles[0], "Episode 1: Introduction");
    assert_eq!(config.titles[1], "Episode 2: Getting Started");

    Ok(())
}

#[tokio::test]
async fn test_config_validation() -> Result<()> {
    let temp_file = create_test_video_file();
    let file_path = temp_file.path().to_string_lossy().to_string();

    // Test valid configuration
    let valid_config = BatchConfigRoot {
        test: false,
        common: CommonConfig {
            prefix: "Test ".to_string(),
            keywords: "test,video".to_string(),
            category: VideoCategory::PeopleBlogs,
            privacy_status: PrivacyStatus::Private,
            playlist_id: "PL1234567890123456".to_string(),
            default_audio_language: "en".to_string(),
            default_language: "en".to_string(),
            recording_date: "2026-01-24".to_string(),
        },
        titles: vec!["Video 1".to_string()],
        files: vec![file_path.clone()],
    };

    assert!(valid_config.validate().is_ok());
    assert!(valid_config.validate_files_and_lengths().await.is_ok());

    // Test invalid configuration - mismatched lengths
    let invalid_config = BatchConfigRoot {
        test: false,
        common: CommonConfig {
            prefix: "Test ".to_string(),
            keywords: "test,video".to_string(),
            category: VideoCategory::PeopleBlogs,
            privacy_status: PrivacyStatus::Private,
            playlist_id: "PL1234567890123456".to_string(),
            default_audio_language: "en".to_string(),
            default_language: "en".to_string(),
            recording_date: "2026-01-24".to_string(),
        },
        titles: vec!["Video 1".to_string(), "Video 2".to_string()],
        files: vec![file_path],
    };

    assert!(invalid_config.validate_files_and_lengths().await.is_err());

    Ok(())
}

#[test]
fn test_playlist_id_validation() {
    use rust_yt_uploader::models::validate_playlist_id;

    // Valid playlist IDs
    assert!(validate_playlist_id("PL1234567890123456").is_ok());
    assert!(validate_playlist_id("PLAbCdEfGhIjKlMnOpQrStUvWxYz").is_ok());
    assert!(validate_playlist_id("PL_-1234567890123456").is_ok());

    // Invalid playlist IDs
    assert!(validate_playlist_id("invalid").is_err());
    assert!(validate_playlist_id("PL123").is_err()); // too short
    assert!(validate_playlist_id("XL1234567890123456").is_err()); // wrong prefix
    assert!(validate_playlist_id("").is_err()); // empty
}

#[test]
fn test_video_category_conversion() -> Result<()> {
    assert_eq!(VideoCategory::PeopleBlogs.as_u32(), 22);
    assert_eq!(VideoCategory::ScienceTechnology.as_u32(), 28);
    assert_eq!(VideoCategory::Gaming.as_u32(), 20);

    assert_eq!(VideoCategory::from_u32(22)?, VideoCategory::PeopleBlogs);
    assert_eq!(
        VideoCategory::from_u32(28)?,
        VideoCategory::ScienceTechnology
    );
    assert_eq!(VideoCategory::from_u32(20)?, VideoCategory::Gaming);

    assert!(VideoCategory::from_u32(999).is_err());

    Ok(())
}

#[test]
fn test_privacy_status_serialization() -> Result<()> {
    let public = PrivacyStatus::Public;
    let private = PrivacyStatus::Private;
    let unlisted = PrivacyStatus::Unlisted;

    let public_json = serde_json::to_string(&public)?;
    let private_json = serde_json::to_string(&private)?;
    let unlisted_json = serde_json::to_string(&unlisted)?;

    assert_eq!(public_json, r#""public""#);
    assert_eq!(private_json, r#""private""#);
    assert_eq!(unlisted_json, r#""unlisted""#);

    let public_deserialized: PrivacyStatus = serde_json::from_str(&public_json)?;
    let private_deserialized: PrivacyStatus = serde_json::from_str(&private_json)?;
    let unlisted_deserialized: PrivacyStatus = serde_json::from_str(&unlisted_json)?;

    assert_eq!(public_deserialized, PrivacyStatus::Public);
    assert_eq!(private_deserialized, PrivacyStatus::Private);
    assert_eq!(unlisted_deserialized, PrivacyStatus::Unlisted);

    Ok(())
}

#[test]
fn test_retry_config() {
    use rust_yt_uploader::RetryConfig;

    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 10);
    assert_eq!(config.base_sleep, 1.0);
    assert_eq!(config.max_sleep, 60.0);
    assert_eq!(config.exponential_base, 2);

    // Test sleep time calculation
    let sleep_time_1 = config.calculate_sleep_time(1);
    let sleep_time_2 = config.calculate_sleep_time(2);
    let sleep_time_10 = config.calculate_sleep_time(10);

    assert!(sleep_time_1 >= 0.0);
    assert!(sleep_time_1 <= config.max_sleep);
    assert!(sleep_time_2 >= 0.0);
    assert!(sleep_time_2 <= config.max_sleep);
    assert!(sleep_time_10 >= 0.0);
    assert!(sleep_time_10 <= config.max_sleep);
}

// Note: The following tests would require actual YouTube API credentials
// and are commented out to avoid failures in CI/CD environments

#[tokio::test]
#[ignore]
async fn test_youtube_authentication() -> Result<()> {
    use rust_yt_uploader::{
        GoogleOAuth, build_youtube_base_url, credentials_path_for_profile, default_youtube_scopes,
        token_path_for_profile,
    };

    // This test requires valid client_secret-test.json and token files
    // Using 'test' profile for testing
    let profile = "test";
    let credentials_path = credentials_path_for_profile(profile)?;
    let token_path = token_path_for_profile(profile)?;
    let client = GoogleOAuth::new(
        credentials_path,
        token_path,
        default_youtube_scopes(),
        build_youtube_base_url(),
    )
    .await;

    assert!(client.is_ok());

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_video_upload() -> Result<()> {
    use rust_yt_uploader::{VideoUploadOptions, YouTubeClient};

    let temp_file = create_test_video_file();
    let file_path = temp_file.path().to_string_lossy().to_string();

    // Using 'test' profile for testing
    let uploader = YouTubeClient::new("test").await?;

    let options = VideoUploadOptions {
        file: file_path,
        title: "Test Upload".to_string(),
        description: "Test video upload from Rust".to_string(),
        keywords: "test,rust,youtube".to_string(),
        category: 28,
        privacy_status: "private".to_string(),
        playlist_id: "PL_YOUR_TEST_PLAYLIST_ID".to_string(),
        default_audio_language: "en".to_string(),
        default_language: "en".to_string(),
        recording_date: "2026-01-24".to_string(),
    };

    let video_id = uploader.upload_video(&options, false).await?;
    assert!(!video_id.is_empty());

    Ok(())
}
