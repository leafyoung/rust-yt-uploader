//! Rust YouTube Uploader Library
//!
//! A Rust-based YouTube video uploader with OAuth 2.0 authentication,
//! supporting both sequential and concurrent upload modes with comprehensive
//! configuration validation.

pub mod google_oauth;
pub mod models;
pub mod progress_stream;
pub mod retry;
pub mod video_process;
pub mod youtube;

// Re-export commonly used types
pub use google_oauth::GoogleOAuth;
pub use models::{
    BatchConfigRoot, CommonConfig, ConfigFormat, IndividualConfigRoot, PrivacyStatus, RetryConfig,
    VideoCategory, VideoConfig, VideoUploadOptions,
};
pub use video_process::merge_videos_with_ffmpeg;
pub use youtube::{
    CaptionDetails, NoProgress, ProgressBarReporter, ProgressReporter, VideoDetails, YouTubeClient,
    upload_batch_concurrent, upload_batch_sequential, upload_individual_sequential,
};
pub use youtube::{
    build_youtube_base_url, build_youtube_direct_upload_url, credentials_path_for_profile,
    default_youtube_scopes, resolve_credentials_path_for_profile, token_path_for_profile,
    validate_profile_name,
};

pub use retry::retry_with_backoff;

/// Initialize tracing/logging with default configuration.
///
/// This function sets up tracing with environment-based log level filtering.
/// Set the `RUST_LOG` environment variable to control the log level (e.g., `RUST_LOG=debug`).
pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .init();
}
