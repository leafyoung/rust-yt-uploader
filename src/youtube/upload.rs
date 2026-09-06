//! Video upload functionality for [`YouTubeClient`].
//!
//! Covers single-video multipart upload plus the individual/batch
//! sequential and concurrent batch drivers.

use anyhow::{Result, anyhow};
use futures::future::try_join_all;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};
use validator::Validate;

use crate::models::{BatchConfigRoot, IndividualConfigRoot, VideoUploadOptions};
use crate::progress_stream::ProgressStream;
use crate::retry::retry_with_backoff;
use crate::video_process::merge_videos_with_ffmpeg;

use super::{ProgressBarReporter, YouTubeClient, build_youtube_direct_upload_url, types};

impl YouTubeClient {
    /// Upload a single video to YouTube
    ///
    /// # Arguments
    /// * `options` - Video upload options
    /// * `test_mode` - If true, delete video and remove from playlist after upload
    pub async fn upload_video(
        &self,
        options: &VideoUploadOptions,
        test_mode: bool,
    ) -> Result<String> {
        let file_path = shellexpand::tilde(&options.file);
        let file_path = Path::new(file_path.as_ref());

        if !file_path.exists() {
            return Err(anyhow!("Video file not found: {}", options.file));
        }

        info!("Starting upload for: {}", options.title);

        let video_id = retry_with_backoff(
            || self.initialize_upload(options),
            self.max_retries,
            self.base_delay_ms,
            "video_upload",
        )
        .await?;

        info!("Video uploaded successfully with ID: {}", video_id);

        let playlist_success = retry_with_backoff(
            || self.add_to_playlist(&video_id, &options.playlist_id),
            self.max_retries,
            self.base_delay_ms,
            "playlist_addition",
        )
        .await;

        match playlist_success {
            Err(ref e) => {
                warn!("Failed to add video to playlist: {}", e);
            }
            Ok(playlist_item_id) => {
                info!(
                    "Video added to playlist successfully {}",
                    options.playlist_id
                );
                if test_mode {
                    info!("Test mode enabled - deleting video after upload, wait for 5 seconds");

                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                    if let Err(e) = self
                        .remove_from_playlist_by_item_id(&playlist_item_id)
                        .await
                    {
                        warn!("Failed to remove video from playlist in test mode: {}", e);
                    }

                    if let Err(e) = self.delete_video(&video_id).await {
                        warn!("Failed to delete video in test mode: {}", e);
                    } else {
                        info!("Successfully deleted video in test mode: {}", video_id);
                    }
                }
            }
        }

        Ok(video_id)
    }

    async fn initialize_upload(&self, options: &VideoUploadOptions) -> Result<String> {
        let file_path = shellexpand::tilde(&options.file);
        let file_path = Path::new(file_path.as_ref());

        let metadata = tokio::fs::metadata(&file_path).await?;
        let file_size = metadata.len();

        self.progress_reporter
            .report_progress(0, file_size, file_path.to_string_lossy().as_ref());

        let tags: Vec<&str> = if options.keywords.is_empty() {
            Vec::new()
        } else {
            options.keywords.split(',').map(|s| s.trim()).collect()
        };

        let metadata_json = json!({
            "snippet": {
                "title": options.title,
                "description": options.description,
                "tags": tags,
                "categoryId": options.category.to_string(),
                "defaultLanguage": options.default_language,
                "defaultAudioLanguage": options.default_audio_language
            },
            "status": {
                "privacyStatus": options.privacy_status
            },
            "recordingDetails": {
                "recordingDate": options.formatted_recording_date()
            }
        });

        use tokio::fs::File;
        use tokio_util::io::ReaderStream;

        // Open file with optimized buffer size for large video uploads
        let file = File::open(&file_path).await?;

        // Use 1MB buffer for optimal throughput with large video files
        // This reduces syscalls and improves I/O efficiency
        const UPLOAD_BUFFER_SIZE: usize = 1024 * 1024; // 1MB
        let stream = ReaderStream::with_capacity(file, UPLOAD_BUFFER_SIZE);

        // Disable bandwidth limiting by default for maximum throughput
        // Bandwidth limit only needed if explicitly requested
        const BANDWIDTH_LIMIT: Option<u64> = None;

        let progress_stream = ProgressStream::new(
            stream,
            file_size,
            self.progress_reporter.clone(),
            file_path.to_string_lossy().to_string(),
            BANDWIDTH_LIMIT,
        );

        let form = reqwest::multipart::Form::new()
            .part(
                "snippet",
                reqwest::multipart::Part::text(metadata_json.to_string())
                    .mime_str("application/json")?,
            )
            .part(
                "media",
                reqwest::multipart::Part::stream_with_length(
                    reqwest::Body::wrap_stream(progress_stream),
                    file_size,
                )
                .mime_str("video/*")?
                .file_name(
                    file_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                ),
            );

        // Upload using multipart
        let response = self
            .client
            .request(reqwest::Method::POST, &build_youtube_direct_upload_url())
            .await?
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to upload video with status {}: {}",
                status,
                text
            ));
        }

        let upload_response: types::VideoUploadResponse = response.json().await?;
        let video_id = upload_response.id;

        self.progress_reporter.report_progress(
            file_size,
            file_size,
            file_path.to_string_lossy().as_ref(),
        );

        self.progress_reporter.finish();

        Ok(video_id)
    }
}

/// Create a client wired to a per-video progress bar sized to the file being uploaded.
///
/// Indicatif hides progress bars automatically when output is not a TTY, so this is
/// safe to use unconditionally.
async fn new_client_with_progress_bar(
    profile: &str,
    title: &str,
    file: &str,
) -> Result<YouTubeClient> {
    let expanded = shellexpand::tilde(file);
    let metadata = tokio::fs::metadata(Path::new(expanded.as_ref())).await?;
    let reporter = Arc::new(ProgressBarReporter::new(title, metadata.len()));
    YouTubeClient::with_progress_reporter(profile, reporter)
        .await
        .map_err(|err| anyhow!("Failed to initialize YouTubeUploader: {}", err))
}

/// Upload videos using individual schema format (sequential).
pub async fn upload_individual_sequential(
    config: IndividualConfigRoot,
    profile: &str,
) -> Result<()> {
    // Validate configuration
    config
        .validate()
        .map_err(|e| anyhow!("Configuration validation failed: {}", e))?;

    info!(
        "Processing {} video(s) using individual schema",
        config.videos.len()
    );

    for (idx, video) in config.videos.iter().enumerate() {
        info!("Processing video {}/{}", idx + 1, config.videos.len());

        let options = VideoUploadOptions {
            file: video.file.clone(),
            title: video.title.clone(), // Individual format uses title as-is
            description: video.description.clone(),
            keywords: video.keywords.clone(),
            category: video.category.as_u32(),
            privacy_status: video.privacy_status.as_ref().to_string(),
            playlist_id: video.playlist_id.clone(),
            default_audio_language: video.default_audio_language.clone(),
            default_language: video.default_language.clone(),
            recording_date: video.recording_date.clone(),
        };

        // Per-video client with a progress bar sized to this file.
        let uploader = new_client_with_progress_bar(profile, &video.title, &video.file).await?;

        match uploader.upload_video(&options, config.test).await {
            Ok(video_id) => info!("Successfully uploaded video: {}", video_id),
            Err(e) => {
                error!("Failed to upload video '{}': {}", video.title, e);
                return Err(e);
            }
        }
    }

    Ok(())
}

/// Upload videos using batch schema format (sequential).
pub async fn upload_batch_sequential(config: BatchConfigRoot, profile: &str) -> Result<()> {
    // Validate configuration
    config
        .validate()
        .map_err(|e| anyhow!("Configuration validation failed: {}", e))?;
    config.validate_files_and_lengths().await?;
    config.common.validate_keywords()?;

    info!(
        "Processing {} video(s) using batch schema",
        config.titles.len()
    );

    let parsed_files = config.parse_files();

    for (idx, (title, file_paths)) in config.titles.iter().zip(parsed_files.iter()).enumerate() {
        info!("Processing video {}/{}", idx + 1, config.titles.len());

        let full_title = format!("{}{}", config.common.prefix, title.trim());

        let (file_to_upload, temp_file_holder) = if file_paths.len() > 1 {
            info!("Merging {} files for video '{}'", file_paths.len(), title);

            let extension = Path::new(file_paths[0].as_str())
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("MTS");
            let suffix = format!(".{}", extension);

            // Use fast temp file creation that prefers /dev/shm
            let temp_file = crate::video_process::create_fast_temp_file(&suffix)?;

            merge_videos_with_ffmpeg(file_paths, temp_file.path()).await?;

            (
                temp_file.path().to_string_lossy().to_string(),
                Some(temp_file),
            )
        } else {
            (file_paths[0].clone(), None)
        };

        let options = VideoUploadOptions {
            file: file_to_upload.clone(),
            title: full_title.clone(),
            description: full_title.clone(), // Use title as description
            keywords: config.common.keywords.clone(),
            category: config.common.category.as_u32(),
            privacy_status: config.common.privacy_status.as_ref().to_string(),
            playlist_id: config.common.playlist_id.clone(),
            default_audio_language: config.common.default_audio_language.clone(),
            default_language: config.common.default_language.clone(),
            recording_date: config.common.recording_date.clone(),
        };

        let uploader = new_client_with_progress_bar(profile, &full_title, &file_to_upload).await?;

        match uploader.upload_video(&options, config.test).await {
            Ok(video_id) => {
                info!("Successfully uploaded video: {}", video_id);
            }
            Err(e) => {
                error!("Failed to upload video '{}': {}", title, e);
                return Err(e);
            }
        }

        drop(temp_file_holder);
    }

    Ok(())
}

/// Upload videos using batch schema format (concurrent).
pub async fn upload_batch_concurrent(
    config: BatchConfigRoot,
    max_concurrent: usize,
    profile: &str,
) -> Result<Vec<String>> {
    // Validate configuration
    config
        .validate()
        .map_err(|e| anyhow!("Configuration validation failed: {}", e))?;
    config.validate_files_and_lengths().await?;
    config.common.validate_keywords()?;

    info!(
        "Uploading {} videos with max {} concurrent",
        config.titles.len(),
        max_concurrent
    );

    let semaphore = Semaphore::new(max_concurrent);

    let parsed_files = config.parse_files();

    let common_keywords = Arc::new(config.common.keywords.clone());
    let common_playlist_id = Arc::new(config.common.playlist_id.clone());
    let common_prefix = Arc::new(config.common.prefix.clone());
    let common_category = config.common.category.as_u32();
    let common_privacy_status = Arc::new(config.common.privacy_status.as_ref().to_string());
    let common_default_audio_language = Arc::new(config.common.default_audio_language.clone());
    let common_default_language = Arc::new(config.common.default_language.clone());
    let common_recording_date = Arc::new(config.common.recording_date.clone());
    let test_mode = config.test;
    let titles_len = config.titles.len();

    let upload_tasks: Vec<_> = config
        .titles
        .iter()
        .zip(parsed_files.iter())
        .enumerate()
        .map(|(idx, (title, file_paths))| {
            let semaphore = &semaphore;
            let title = title.clone();
            let file_paths = file_paths.clone();
            let keywords = common_keywords.clone();
            let playlist_id = common_playlist_id.clone();
            let prefix = common_prefix.clone();
            let privacy_status = common_privacy_status.clone();
            let default_audio_language = common_default_audio_language.clone();
            let default_language = common_default_language.clone();
            let recording_date = common_recording_date.clone();

            async move {
                let _permit = semaphore.acquire().await.unwrap();

                info!("Starting upload {}/{}: {}", idx + 1, titles_len, title);

                let full_title = format!("{}{}", prefix, title.trim());

                // Merge files if there are multiple
                let (file_to_upload, temp_file_holder) = if file_paths.len() > 1 {
                    info!("Merging {} files for video '{}'", file_paths.len(), title);

                    let extension = Path::new(file_paths[0].as_str())
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("MTS");
                    let suffix = format!(".{}", extension);
                    // Use fast temp file creation that prefers /dev/shm
                    let temp_file = crate::video_process::create_fast_temp_file(&suffix)?;

                    merge_videos_with_ffmpeg(&file_paths, temp_file.path()).await?;

                    (
                        temp_file.path().to_string_lossy().to_string(),
                        Some(temp_file),
                    )
                } else {
                    (file_paths[0].clone(), None)
                };

                // Per-video client + progress bar; create before `full_title`
                // is moved into the upload options below.
                let uploader =
                    new_client_with_progress_bar(profile, &full_title, &file_to_upload).await?;

                let options = VideoUploadOptions {
                    file: file_to_upload.clone(),
                    title: full_title.clone(),
                    description: full_title,
                    keywords: keywords.to_string(),
                    category: common_category,
                    privacy_status: privacy_status.to_string(),
                    playlist_id: playlist_id.to_string(),
                    default_audio_language: default_audio_language.to_string(),
                    default_language: default_language.to_string(),
                    recording_date: recording_date.to_string(),
                };

                let result = uploader.upload_video(&options, test_mode).await;

                drop(temp_file_holder);

                match &result {
                    Ok(video_id) => {
                        info!("Completed upload {}/{}: {}", idx + 1, titles_len, video_id)
                    }
                    Err(e) => error!("Failed upload {}/{}: {}", idx + 1, titles_len, e),
                }

                result
            }
        })
        .collect();

    let results = try_join_all(upload_tasks).await?;
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CommonConfig, PrivacyStatus, VideoCategory, VideoConfig};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_video_file() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "fake video content").unwrap();
        file
    }

    #[test]
    fn test_video_upload_options_creation() {
        let options = VideoUploadOptions {
            file: "test.mp4".to_string(),
            title: "Test Video".to_string(),
            description: "Test Description".to_string(),
            keywords: "test,video".to_string(),
            category: VideoCategory::PeopleBlogs.as_u32(),
            privacy_status: "private".to_string(),
            playlist_id: "PL1234567890123456".to_string(),
            default_audio_language: "en".to_string(),
            default_language: "en".to_string(),
            recording_date: "2026-01-24T00:00:00.000Z".to_string(),
        };

        assert_eq!(options.title, "Test Video");
        assert_eq!(options.category, 22);
        assert_eq!(options.privacy_status, "private");
    }

    #[tokio::test]
    async fn test_individual_config_validation() {
        let temp_file = create_test_video_file();
        let file_path = temp_file.path().to_string_lossy().to_string();

        let video_config = VideoConfig {
            title: "Test Video".to_string(),
            description: "Test Description".to_string(),
            keywords: "test,video".to_string(),
            file: file_path,
            category: VideoCategory::PeopleBlogs,
            privacy_status: PrivacyStatus::Private,
            playlist_id: "PL1234567890123456".to_string(),
            default_audio_language: "en".to_string(),
            default_language: "en".to_string(),
            recording_date: "2026-01-24T00:00:00.000Z".to_string(),
        };

        let config = IndividualConfigRoot {
            test: false,
            videos: vec![video_config],
        };

        // This should not panic
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn test_batch_config_validation() {
        let temp_file = create_test_video_file();
        let file_path = temp_file.path().to_string_lossy().to_string();

        let common_config = CommonConfig {
            prefix: "Test Prefix ".to_string(),
            keywords: "test,video".to_string(),
            category: VideoCategory::PeopleBlogs,
            privacy_status: PrivacyStatus::Private,
            playlist_id: "PL1234567890123456".to_string(),
            default_audio_language: "en".to_string(),
            default_language: "en".to_string(),
            recording_date: "2026-01-24T00:00:00.000Z".to_string(),
        };

        let config = BatchConfigRoot {
            test: false,
            common: common_config,
            titles: vec!["Video 1".to_string()],
            files: vec![file_path],
        };

        assert!(config.validate().is_ok());
        assert!(config.validate_files_and_lengths().await.is_ok());
    }
}
