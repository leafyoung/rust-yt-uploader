//! Shared test utilities for the Rust YouTube uploader.
//!
//! This module provides common test fixtures and helper functions
//! used across multiple test files.

use std::io::Write;
use tempfile::NamedTempFile;

/// Create a temporary video file for testing
pub fn create_test_video_file() -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file, "fake video content").expect("Failed to write to temp file");
    file
}
