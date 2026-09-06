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

// Re-exports consumed by the CLI binaries and external embedders.
// Everything else stays accessible through its home module
// (`models::`, `youtube::`, `google_oauth::`, `retry::`, `video_process::`).
pub use models::{BatchConfigRoot, ConfigFormat, IndividualConfigRoot};
pub use youtube::{
    CaptionDetails, VideoDetails, YouTubeClient, upload_batch_concurrent, upload_batch_sequential,
    upload_individual_sequential, validate_profile_name,
};

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
        .init();
}
