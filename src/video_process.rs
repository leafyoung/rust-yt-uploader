//! Video processing utilities for YouTube uploader.

use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::process::Command;
use tokio::task::spawn_blocking;

/// Get optimal temp directory - prefer fast storage like /dev/shm if available
fn get_fast_temp_dir() -> Option<PathBuf> {
    // Check for in-memory tmpfs which is much faster than disk
    let candidates = ["/dev/shm", "/tmp", "/var/tmp"];

    for dir in &candidates {
        let path = Path::new(dir);
        if path.exists() && path.is_dir() {
            // Check if it's writable
            if std::fs::metadata(path)
                .map(|m| !m.permissions().readonly())
                .unwrap_or(false)
            {
                return Some(path.to_path_buf());
            }
        }
    }
    None
}

/// Create a temp file in the fastest available location
///
/// Prefers in-memory tmpfs (/dev/shm) for speed, falling back to system temp
pub fn create_fast_temp_file(suffix: &str) -> Result<NamedTempFile> {
    let mut builder = tempfile::Builder::new();
    builder.suffix(suffix);

    if let Some(fast_dir) = get_fast_temp_dir() {
        match builder.tempfile_in(&fast_dir) {
            Ok(file) => {
                tracing::debug!("Created temp file in fast directory: {:?}", fast_dir);
                return Ok(file);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to create temp file in {:?}: {}, falling back to default",
                    fast_dir,
                    e
                );
            }
        }
    }

    // Fallback to default temp directory
    Ok(builder.tempfile()?)
}

/// Merge multiple video files using ffmpeg concat demuxer with optimized settings
///
/// This is an async function that runs ffmpeg in a non-blocking way,
/// preventing the async runtime from being blocked during video processing.
///
/// # Arguments
/// * `files` - Slice of file paths to merge
/// * `output_file` - Path for the merged output file
///
/// # Returns
/// * Result indicating success or failure
pub async fn merge_videos_with_ffmpeg(files: &[String], output_file: &Path) -> Result<()> {
    tracing::info!("Merging {} video files using ffmpeg", files.len());
    let start = std::time::Instant::now();

    // Create concat list file in fast temp directory
    // Use spawn_blocking for temp file creation since it does blocking I/O
    let concat_file = spawn_blocking(|| create_fast_temp_file(".txt")).await??;

    // Write concat list using tokio::fs for async file I/O
    let concat_path = concat_file.path().to_path_buf();
    let mut content = String::new();
    for file_path in files {
        content.push_str(&format!("file '{}'\n", file_path));
    }
    tokio::fs::write(&concat_path, content).await?;

    let concat_file_path = concat_path.to_string_lossy().to_string();

    // Build optimized ffmpeg command
    // -threads 0: auto-detect optimal thread count
    // -fflags +genpts: generate presentation timestamps (improves compatibility)
    // -avoid_negative_ts make_zero: fix timestamp issues
    let output_path = output_file.to_string_lossy().to_string();
    let status = Command::new("ffmpeg")
        .arg("-y")
        .arg("-threads")
        .arg("0")
        .arg("-fflags")
        .arg("+genpts")
        .arg("-safe")
        .arg("0")
        .arg("-f")
        .arg("concat")
        .arg("-i")
        .arg(&concat_file_path)
        .arg("-c")
        .arg("copy")
        .arg("-avoid_negative_ts")
        .arg("make_zero")
        .arg(&output_path)
        // Use process group to ensure clean termination
        .kill_on_drop(true)
        .status()
        .await?;

    if !status.success() {
        return Err(anyhow!("ffmpeg failed with status: {}", status));
    }

    let elapsed = start.elapsed();
    tracing::info!(
        "Successfully merged videos to {} in {:.2}s",
        output_path,
        elapsed.as_secs_f64()
    );
    Ok(())
}

/// Check if ffmpeg is available on the system
///
/// # Returns
/// * true if ffmpeg is available, false otherwise
pub async fn check_ffmpeg_available() -> bool {
    match Command::new("ffmpeg").arg("-version").output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Get video duration in seconds using ffprobe
///
/// # Arguments
/// * `video_path` - Path to the video file
///
/// # Returns
/// * Result containing duration in seconds
pub async fn get_video_duration(video_path: &Path) -> Result<f64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(video_path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(anyhow!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let duration: f64 = duration_str.trim().parse()?;

    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ffmpeg_available() {
        // This test just checks if ffmpeg is available
        let available = check_ffmpeg_available().await;
        println!("ffmpeg available: {}", available);
        // Don't assert - ffmpeg might not be installed in CI
    }

    #[test]
    fn test_fast_temp_dir() {
        let dir = get_fast_temp_dir();
        println!("Fast temp dir: {:?}", dir);
        // Should return Some path on most systems
        assert!(dir.is_some(), "Should find a temp directory");
    }

    #[test]
    fn test_create_fast_temp_file_success() {
        let temp_file = create_fast_temp_file(".mkv").expect("Should create temp file");
        let path = temp_file.path();

        // Check file exists
        assert!(path.exists(), "Temp file should exist");
        assert!(
            path.to_string_lossy().ends_with(".mkv"),
            "Should have correct suffix"
        );

        // Check parent directory exists and is writable
        let parent = path.parent().expect("Should have parent directory");
        assert!(parent.exists(), "Parent directory should exist");

        println!("Created temp file at: {:?}", path);

        // File is automatically cleaned up when temp_file is dropped
    }

    #[test]
    fn test_create_fast_temp_file_different_suffixes() {
        let suffixes = vec![".txt", ".mkv", ".mp4", ".tmp", ""];

        for suffix in suffixes {
            let temp_file = create_fast_temp_file(suffix).expect("Should create temp file");
            let path = temp_file.path();
            assert!(
                path.exists(),
                "Temp file should exist for suffix: {}",
                suffix
            );
            if !suffix.is_empty() {
                assert!(
                    path.to_string_lossy().ends_with(suffix),
                    "Should have correct suffix: {}",
                    suffix
                );
            }
        }
    }

    #[tokio::test]
    async fn test_get_video_duration_invalid_file() {
        // Test with a non-existent file
        let result = get_video_duration(Path::new("/nonexistent/file.mp4")).await;
        assert!(result.is_err(), "Should fail for non-existent file");
    }

    #[test]
    fn test_get_fast_temp_dir_prefers_dev_shm() {
        // If /dev/shm exists, it should be preferred
        let dir = get_fast_temp_dir();

        if Path::new("/dev/shm").exists() {
            // On systems with /dev/shm, it should be preferred
            assert_eq!(
                dir,
                Some(PathBuf::from("/dev/shm")),
                "Should prefer /dev/shm"
            );
        }
    }
}
