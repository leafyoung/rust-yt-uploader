//! Video processing utilities for YouTube uploader.

use anyhow::{Result, anyhow};
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

/// Merge multiple video files using ffmpeg concat demuxer
///
/// # Arguments
/// * `files` - Slice of file paths to merge
/// * `output_file` - Path for the merged output file
///
/// # Returns
/// * Result indicating success or failure
pub fn merge_videos_with_ffmpeg(files: &[String], output_file: &Path) -> Result<()> {
    tracing::info!("Merging {} video files using ffmpeg", files.len());

    // Create a temporary file with the concat list (auto-deleted when function returns)
    let concat_file = NamedTempFile::new()?;
    {
        let mut file = concat_file.as_file();
        for file_path in files {
            writeln!(file, "file '{}'", file_path)?;
        }
        file.flush()?;
    }

    let concat_file_path = concat_file.path().to_string_lossy().to_string();

    // Build ffmpeg command
    let output_path = output_file.to_string_lossy().to_string();
    let status = std::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-safe")
        .arg("0")
        .arg("-f")
        .arg("concat")
        .arg("-i")
        .arg(&concat_file_path)
        .arg("-c")
        .arg("copy")
        .arg(&output_path)
        .status()?;

    if !status.success() {
        return Err(anyhow!("ffmpeg failed with status: {}", status));
    }

    tracing::info!("Successfully merged videos to {}", output_path);
    Ok(())
}
