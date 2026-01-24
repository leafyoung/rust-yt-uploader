//! Configuration models for YouTube uploader with validation.
//!
//! This module provides Serde-based models that mirror the Python Pydantic models,
//! supporting both individual and batch YAML configuration formats.

use anyhow::{Result, anyhow};
use futures::future::try_join_all;
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use validator::{Validate, ValidationError};

/// Configuration format detection result
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigFormat {
    Individual,
    Batch,
}

/// Configuration for retry behavior during uploads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base sleep time in seconds
    pub base_sleep: f64,
    /// Maximum sleep time in seconds
    pub max_sleep: f64,
    /// Exponential backoff base
    pub exponential_base: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 10,
            base_sleep: 1.0,
            max_sleep: 60.0,
            exponential_base: 2,
        }
    }
}

impl RetryConfig {
    /// Calculate sleep time using exponential backoff with jitter.
    ///
    /// # Arguments
    /// * `retry_attempt` - The current retry attempt number (1-based)
    ///
    /// # Returns
    /// * Sleep time in seconds, capped at max_sleep
    pub fn calculate_sleep_time(&self, retry_attempt: u32) -> f64 {
        let exponential_sleep = (self.exponential_base.pow(retry_attempt)) as f64;
        let sleep_time = rand::rng().random::<f64>() * exponential_sleep;
        sleep_time.min(self.max_sleep)
    }
}

/// Options for uploading a single video.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoUploadOptions {
    pub file: String,
    pub title: String,
    pub description: String,
    pub keywords: String,
    pub category: u32,
    pub privacy_status: String,
    pub playlist_id: String,
    #[serde(rename = "defaultAudioLanguage")]
    pub default_audio_language: String,
    #[serde(rename = "defaultLanguage")]
    pub default_language: String,
    #[serde(rename = "recordingDate")]
    pub recording_date: String,
}

impl VideoUploadOptions {
    /// Convert recording_date from "YYYY-MM-DD" to YouTube API timestamp format "YYYY-MM-DDTHH:MM:SS.000Z"
    pub fn formatted_recording_date(&self) -> String {
        if self.recording_date.contains('T') {
            // Already in timestamp format, return as-is
            self.recording_date.clone()
        } else {
            // Convert "YYYY-MM-DD" to "YYYY-MM-DDT00:00:00.000Z"
            format!("{}T00:00:00.000Z", self.recording_date)
        }
    }
}

/// Valid YouTube video privacy status values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PrivacyStatus {
    Public,
    #[default]
    Private,
    Unlisted,
}

impl From<&str> for PrivacyStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "public" => PrivacyStatus::Public,
            "unlisted" => PrivacyStatus::Unlisted,
            _ => PrivacyStatus::Private,
        }
    }
}

impl AsRef<str> for PrivacyStatus {
    fn as_ref(&self) -> &str {
        match self {
            PrivacyStatus::Public => "public",
            PrivacyStatus::Unlisted => "unlisted",
            PrivacyStatus::Private => "private",
        }
    }
}

/// YouTube video category IDs.
///
/// See: <https://developers.google.com/youtube/v3/docs/videoCategories/list>
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[repr(u32)]
pub enum VideoCategory {
    FilmAnimation = 1,
    AutosVehicles = 2,
    Music = 10,
    PetsAnimals = 15,
    Sports = 17,
    ShortMovies = 18,
    TravelEvents = 19,
    Gaming = 20,
    Videoblogging = 21,
    #[default]
    PeopleBlogs = 22,
    Comedy = 23,
    Entertainment = 24,
    NewsPolitics = 25,
    HowtoStyle = 26,
    Education = 27,
    ScienceTechnology = 28,
    NonprofitsActivism = 29,
    Movies = 30,
    AnimationAnime = 31,
    ActionAdventure = 32,
    Classics = 33,
    ComedyFilm = 34,
    Documentary = 35,
    Drama = 36,
    Family = 37,
    Foreign = 38,
    Horror = 39,
    SciFiFantasy = 40,
    Thriller = 41,
    Shorts = 42,
    Shows = 43,
}

impl VideoCategory {
    /// Convert to u32 value for YouTube API
    pub fn as_u32(&self) -> u32 {
        *self as u32
    }

    /// Create from u32 value
    pub fn from_u32(value: u32) -> Result<Self> {
        match value {
            1 => Ok(Self::FilmAnimation),
            2 => Ok(Self::AutosVehicles),
            10 => Ok(Self::Music),
            15 => Ok(Self::PetsAnimals),
            17 => Ok(Self::Sports),
            18 => Ok(Self::ShortMovies),
            19 => Ok(Self::TravelEvents),
            20 => Ok(Self::Gaming),
            21 => Ok(Self::Videoblogging),
            22 => Ok(Self::PeopleBlogs),
            23 => Ok(Self::Comedy),
            24 => Ok(Self::Entertainment),
            25 => Ok(Self::NewsPolitics),
            26 => Ok(Self::HowtoStyle),
            27 => Ok(Self::Education),
            28 => Ok(Self::ScienceTechnology),
            29 => Ok(Self::NonprofitsActivism),
            30 => Ok(Self::Movies),
            31 => Ok(Self::AnimationAnime),
            32 => Ok(Self::ActionAdventure),
            33 => Ok(Self::Classics),
            34 => Ok(Self::ComedyFilm),
            35 => Ok(Self::Documentary),
            36 => Ok(Self::Drama),
            37 => Ok(Self::Family),
            38 => Ok(Self::Foreign),
            39 => Ok(Self::Horror),
            40 => Ok(Self::SciFiFantasy),
            41 => Ok(Self::Thriller),
            42 => Ok(Self::Shorts),
            43 => Ok(Self::Shows),
            _ => Err(anyhow!("Invalid video category ID: {}", value)),
        }
    }
}

/// Custom validation function for playlist ID
pub fn validate_playlist_id(playlist_id: &str) -> Result<(), ValidationError> {
    let re = Regex::new(r"^PL[a-zA-Z0-9_-]{16,33}$").unwrap();
    if re.is_match(playlist_id) {
        Ok(())
    } else {
        Err(ValidationError::new("Invalid playlist ID format"))
    }
}

/// Custom validation function for file existence
fn validate_file_exists(file_path: &str) -> Result<(), ValidationError> {
    let expanded_path = shellexpand::tilde(file_path);
    if Path::new(expanded_path.as_ref()).exists() {
        Ok(())
    } else {
        Err(ValidationError::new("File does not exist"))
    }
}

/// Common configuration shared across multiple videos.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CommonConfig {
    /// Title prefix for all videos
    #[validate(length(min = 1))]
    pub prefix: String,

    /// Comma-separated keywords/tags
    #[validate(length(min = 1))]
    pub keywords: String,

    /// Video category
    #[serde(default)]
    pub category: VideoCategory,

    /// Privacy status
    #[serde(default, rename = "privacyStatus")]
    pub privacy_status: PrivacyStatus,

    /// Playlist ID
    #[validate(custom(function = "validate_playlist_id"))]
    #[serde(rename = "playlistId")]
    pub playlist_id: String,

    /// Default audio language for the video
    #[serde(rename = "defaultAudioLanguage")]
    pub default_audio_language: String,

    /// Default language for the video
    #[serde(rename = "defaultLanguage")]
    pub default_language: String,

    /// Recording date for the video
    #[serde(rename = "recordingDate")]
    pub recording_date: String,
}

impl CommonConfig {
    /// Validate keywords are not empty or whitespace only
    pub fn validate_keywords(&self) -> Result<()> {
        if self.keywords.trim().is_empty() {
            return Err(anyhow!("keywords cannot be empty or whitespace only"));
        }
        Ok(())
    }
}

/// Configuration for a single video (individual format).
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct VideoConfig {
    /// Video title
    #[validate(length(min = 1, max = 100))]
    pub title: String,

    /// Video description
    #[serde(default)]
    pub description: String,

    /// Comma-separated keywords/tags
    #[validate(length(min = 1))]
    pub keywords: String,

    /// Path to video file
    #[validate(length(min = 1), custom(function = "validate_file_exists"))]
    pub file: String,

    /// Video category
    pub category: VideoCategory,

    /// Privacy status
    #[serde(rename = "privacyStatus")]
    pub privacy_status: PrivacyStatus,

    /// Playlist ID
    #[validate(custom(function = "validate_playlist_id"))]
    #[serde(rename = "playlistId")]
    pub playlist_id: String,

    /// Default audio language for the video
    #[serde(rename = "defaultAudioLanguage")]
    pub default_audio_language: String,

    /// Default language for the video
    #[serde(rename = "defaultLanguage")]
    pub default_language: String,

    /// Recording date for the video
    #[serde(rename = "recordingDate")]
    pub recording_date: String,
}

/// Root model for individual YAML format with videos array.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IndividualConfigRoot {
    /// Test mode flag - if true, delete videos after upload
    #[serde(default = "bool::default")]
    pub test: bool,

    /// List of video configurations
    #[validate(length(min = 1), nested)]
    pub videos: Vec<VideoConfig>,
}

/// Root model for batch YAML format with common config.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct BatchConfigRoot {
    /// Test mode flag - if true, delete videos after upload
    #[serde(default = "bool::default")]
    pub test: bool,

    /// Common configuration for all videos
    #[validate(nested)]
    pub common: CommonConfig,

    /// List of video titles
    #[validate(length(min = 1))]
    pub titles: Vec<String>,

    /// List of video file paths
    #[validate(length(min = 1))]
    pub files: Vec<String>,
}

impl BatchConfigRoot {
    /// Validate that files exist and titles/files have matching lengths
    pub async fn validate_files_and_lengths(&self) -> Result<()> {
        // Check that titles and files have same length
        if self.titles.len() != self.files.len() {
            return Err(anyhow!(
                "Mismatch between titles and files: {} titles != {} files",
                self.titles.len(),
                self.files.len()
            ));
        }

        // Check that all files exist (in parallel)
        let file_checks: Vec<_> = self
            .files
            .iter()
            .map(|file_path| async move { validate_file_exists(file_path) })
            .collect();

        try_join_all(file_checks).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 10);
        assert_eq!(config.base_sleep, 1.0);
        assert_eq!(config.max_sleep, 60.0);
        assert_eq!(config.exponential_base, 2);
    }

    #[test]
    fn test_retry_config_calculate_sleep_time() {
        let config = RetryConfig::default();
        let sleep_time = config.calculate_sleep_time(1);
        assert!(sleep_time >= 0.0);
        assert!(sleep_time <= config.max_sleep);
    }

    #[test]
    fn test_video_category_conversion() {
        assert_eq!(VideoCategory::PeopleBlogs.as_u32(), 22);
        assert_eq!(
            VideoCategory::from_u32(22).unwrap(),
            VideoCategory::PeopleBlogs
        );
        assert!(VideoCategory::from_u32(999).is_err());
    }

    #[test]
    fn test_playlist_id_validation() {
        assert!(validate_playlist_id("PL1234567890123456").is_ok());
        assert!(validate_playlist_id("PLAbCdEfGhIjKlMnOpQrStUvWxYz").is_ok());
        assert!(validate_playlist_id("invalid").is_err());
        assert!(validate_playlist_id("PL123").is_err()); // too short
    }

    #[test]
    fn test_formatted_recording_date() {
        let options = VideoUploadOptions {
            file: "test.mp4".to_string(),
            title: "Test".to_string(),
            description: "Test".to_string(),
            keywords: "test".to_string(),
            category: 22,
            privacy_status: "private".to_string(),
            playlist_id: "PL1234567890123456".to_string(),
            default_audio_language: "en".to_string(),
            default_language: "en".to_string(),
            recording_date: "2026-01-24".to_string(),
        };

        // Should convert YYYY-MM-DD to YYYY-MM-DDT00:00:00.000Z
        assert_eq!(
            options.formatted_recording_date(),
            "2026-01-24T00:00:00.000Z"
        );

        // Already in timestamp format should remain unchanged
        let options_with_timestamp = VideoUploadOptions {
            recording_date: "2026-01-24T12:30:45.000Z".to_string(),
            ..options
        };
        assert_eq!(
            options_with_timestamp.formatted_recording_date(),
            "2026-01-24T12:30:45.000Z"
        );
    }
}
