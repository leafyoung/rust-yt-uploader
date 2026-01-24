//! Rust YouTube Uploader Library
//!
//! A Rust-based YouTube video uploader with OAuth 2.0 authentication,
//! supporting both sequential and concurrent upload modes with comprehensive
//! configuration validation.

pub mod google_oauth;
pub mod models;
pub mod retry;
pub mod youtube_client;

// Re-export commonly used types
pub use google_oauth::{Credentials, GoogleOAuth};
pub use models::{
    BatchConfigRoot, CommonConfig, ConfigFormat, IndividualConfigRoot, PrivacyStatus, RetryConfig,
    VideoCategory, VideoConfig, VideoUploadOptions,
};
pub use youtube_client::{
    VideoDetails, YouTubeClient, upload_batch_concurrent, upload_batch_sequential,
    upload_individual_sequential,
};

pub use retry::retry_with_backoff;
