//! YouTube video upload functionality.
//!
//! This module provides the core video upload functionality, including
//! direct uploads, playlist management, and both sequential and concurrent
//! upload modes.

use anyhow::{Result, anyhow};
use futures::future::try_join_all;
use futures::stream::{self, Stream, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use crate::google_oauth::GoogleOAuth;
use crate::models::{BatchConfigRoot, IndividualConfigRoot, RetryConfig, VideoUploadOptions};
use crate::progress_stream::ProgressStream;
use crate::retry::retry_with_backoff;
use crate::video_process::merge_videos_with_ffmpeg;
use crate::youtube::{
    build_youtube_base_url, build_youtube_direct_upload_url, default_youtube_scopes,
    resolve_credentials_path_for_profile, token_path_for_profile, types,
};
use reqwest::RequestBuilder;
use validator::Validate;

/// Progress reporter trait for upload progress tracking
pub trait ProgressReporter: Send + Sync {
    fn report_progress(&self, uploaded: u64, total: u64, filename: &str);
    fn finish(&self);
}

/// No-op progress reporter that does nothing
pub struct NoProgress;

impl ProgressReporter for NoProgress {
    fn report_progress(&self, _uploaded: u64, _total: u64, _filename: &str) {}
    fn finish(&self) {}
}

/// Progress bar implementation using indicatif
pub struct ProgressBarReporter {
    bar: ProgressBar,
}

impl ProgressBarReporter {
    pub fn new(filename: &str, total_size: u64) -> Self {
        let bar = ProgressBar::new(total_size);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta}) {msg}"
            )
            .unwrap()
            .progress_chars("#>-")
        );
        bar.set_message(format!("Uploading {}", filename));

        Self { bar }
    }
}

impl ProgressReporter for ProgressBarReporter {
    fn report_progress(&self, uploaded: u64, _total: u64, _filename: &str) {
        self.bar.set_position(uploaded);
    }

    fn finish(&self) {
        self.bar.finish_with_message("Upload complete");
    }
}

/// Video details for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDetails {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub upload_date: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    pub tags: Vec<String>,
    #[serde(rename = "defaultLanguage")]
    pub default_language: Option<String>,
    #[serde(rename = "defaultAudioLanguage")]
    pub default_audio_language: Option<String>,
    #[serde(rename = "recordingDate")]
    pub recording_date: Option<String>,
    pub duration: Option<String>,
    pub caption: Option<String>,
}

/// Caption/subtitle details for a video
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionDetails {
    pub id: String,
    #[serde(rename = "videoId")]
    pub video_id: String,
    pub language: String,
    #[serde(rename = "isAutoSynced")]
    pub is_auto_synced: Option<bool>,
    #[serde(rename = "isCC")]
    pub is_cc: Option<bool>,
    #[serde(rename = "isLarge")]
    pub is_large: Option<bool>,
    #[serde(rename = "isDraft")]
    pub is_draft: Option<bool>,
    pub name: Option<String>,
    #[serde(rename = "audioTrackType")]
    pub audio_track_type: Option<String>,
    #[serde(rename = "isEasyReader")]
    pub is_easy_reader: Option<bool>,
}

/// YouTube video uploader
pub struct YouTubeClient {
    client: GoogleOAuth,
    retry_config: RetryConfig,
    progress_reporter: Arc<dyn ProgressReporter>,
}

impl YouTubeClient {
    /// Create a new YouTube uploader with a specific profile
    ///
    /// # Arguments
    /// * `profile` - Profile name (alphanumeric only).
    ///   Credentials will be loaded from `client_secret-{profile}.json`
    ///   Token will be saved to/loaded from `youtube-oauth2-{profile}.json`
    ///
    /// # Errors
    /// * Returns error if profile name contains invalid characters
    pub async fn new(profile: &str) -> Result<Self> {
        let credentials_path = resolve_credentials_path_for_profile(profile)?;
        let token_path = token_path_for_profile(profile)?;
        Self::with_credentials_path(credentials_path, token_path).await
    }

    /// Create a new YouTube uploader with a custom credentials path
    pub async fn with_credentials_path<P: AsRef<Path>>(
        credentials_path: P,
        token_path: P,
    ) -> Result<Self> {
        let scopes = default_youtube_scopes();

        let client = GoogleOAuth::new(
            credentials_path,
            token_path,
            scopes,
            build_youtube_base_url(),
        )
        .await?;
        let retry_config = RetryConfig::default();

        Ok(Self {
            client,
            retry_config,
            progress_reporter: Arc::new(NoProgress),
        })
    }

    /// Create a new YouTube uploader with profile and custom progress reporter
    ///
    /// # Arguments
    /// * `profile` - Profile name (alphanumeric only).
    ///   Credentials will be loaded from `client_secret-{profile}.json`
    ///   Token will be saved to/loaded from `youtube-oauth2-{profile}.json`
    /// * `progress_reporter` - Custom progress reporter implementation
    pub async fn with_progress_reporter(
        profile: &str,
        progress_reporter: Arc<dyn ProgressReporter>,
    ) -> Result<Self> {
        let credentials_path = resolve_credentials_path_for_profile(profile)?;
        let token_path = token_path_for_profile(profile)?;
        let scopes = default_youtube_scopes();

        let client = GoogleOAuth::new(
            credentials_path,
            token_path,
            scopes,
            build_youtube_base_url(),
        )
        .await?;
        let retry_config = RetryConfig::default();

        Ok(Self {
            client,
            retry_config,
            progress_reporter,
        })
    }

    /// Execute an HTTP request and parse the JSON response, with standardized error handling.
    ///
    /// This helper eliminates duplicate error handling code across API methods.
    ///
    /// # Arguments
    /// * `request` - The request builder to execute
    /// * `operation` - Description of the operation for error messages
    ///
    /// # Returns
    /// * The deserialized response type T
    async fn execute_and_parse<T>(&self, request: RequestBuilder, operation: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "YouTube API {} failed with status {}: {}",
                operation,
                status,
                text
            ));
        }
        Ok(response.json().await?)
    }

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
            &self.retry_config,
            "video_upload",
        )
        .await?;

        info!("Video uploaded successfully with ID: {}", video_id);

        let playlist_success = retry_with_backoff(
            || self.add_to_playlist(&video_id, &options.playlist_id),
            &self.retry_config,
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
    async fn add_to_playlist(&self, video_id: &str, playlist_id: &str) -> Result<String> {
        let playlist_info = self
            .client
            .get(&format!(
                "playlistItems?part=contentDetails&playlistId={}",
                playlist_id
            ))
            .await?
            .send()
            .await?;

        if !playlist_info.status().is_success() {
            let status = playlist_info.status();
            let text = playlist_info.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to get playlist info with status {}: {}",
                status,
                text
            ));
        }

        // {
        //  "etag": String,
        //  "items": Array [
        //      {
        //          "contentDetails": {
        //          "videoId": String,
        //          "videoPublishedAt": String,
        //          "etag": String,
        //          "id": String,
        //          "kind": String },
        //      } ],
        //  "kind": String,
        //  "pageInfo": {"resultsPerPage": Number(5), "totalResults": Number(3)}
        // }

        let info: serde_json::Value = playlist_info.json().await?;
        let info_response: types::PlaylistInfoResponse = serde_json::from_value(info)?;
        let position = info_response.page_info.total_results;

        info!("Adding video to playlist at position {}", position);

        let playlist_item = json!({
            "snippet": {
                "playlistId": playlist_id,
                "position": position,
                "resourceId": {
                    "kind": "youtube#video",
                    "videoId": video_id
                }
            }
        });

        let response = self
            .client
            .post("playlistItems?part=snippet")
            .await?
            .json(&playlist_item)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to add to playlist with status {}: {}",
                status,
                text
            ));
        }

        let playlist_response: types::PlaylistItemResponse = response.json().await?;
        info!(
            "Added video to playlist at position {:?}",
            playlist_response.snippet.position
        );

        Ok(playlist_response.id)
    }

    /// Delete a video by its ID.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID to delete
    ///
    /// # Returns
    /// * Result indicating success or failure
    ///
    /// # API Endpoint
    /// DELETE <https://www.googleapis.com/youtube/v3/videos?id={video_id}>
    pub async fn delete_video(&self, video_id: &str) -> Result<()> {
        info!("Deleting video with ID: {}", video_id);

        let response = self
            .client
            .delete(&format!("videos?id={}", video_id))
            .await?
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to delete video with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully deleted video: {}", video_id);
        Ok(())
    }

    /// Remove a video from a playlist by playlist item ID.
    ///
    /// # Arguments
    /// * `playlist_item_id` - The playlist item ID to remove
    ///
    /// # Returns
    /// * Result indicating success or failure
    ///
    /// # API Endpoint
    /// DELETE <https://www.googleapis.com/youtube/v3/playlistItems?id={playlist_item_id}>
    pub async fn remove_from_playlist_by_item_id(&self, playlist_item_id: &str) -> Result<()> {
        info!("Removing playlist item with ID: {}", playlist_item_id);

        let response = self
            .client
            .delete(&format!("playlistItems?id={}", playlist_item_id))
            .await?
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to remove playlist item with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully removed playlist item: {}", playlist_item_id);
        Ok(())
    }

    /// Remove a video from a playlist by video ID.
    ///
    /// This method first queries the playlist to find the playlist item ID
    /// associated with the video, then removes that item.
    ///
    /// # Arguments
    /// * `playlist_id` - The playlist ID
    /// * `video_id` - The YouTube video ID to remove from the playlist
    ///
    /// # Returns
    /// * Result indicating success or failure
    ///
    /// # API Endpoints
    /// 1. GET <https://www.googleapis.com/youtube/v3/playlistItems?part=id&playlistId={playlist_id}&videoId={video_id}>
    /// 2. DELETE <https://www.googleapis.com/youtube/v3/playlistItems?id={playlist_item_id}>
    pub async fn remove_from_playlist_by_video_id(
        &self,
        playlist_id: &str,
        video_id: &str,
    ) -> Result<()> {
        info!("Removing video {} from playlist {}", video_id, playlist_id);

        let playlist_items: types::PlaylistItemsResponse = self
            .execute_and_parse(
                self.client
                    .get(&format!(
                        "playlistItems?part=id&playlistId={}&videoId={}",
                        playlist_id, video_id
                    ))
                    .await?,
                "query playlist items",
            )
            .await?;

        if playlist_items.items.is_empty() {
            return Err(anyhow!(
                "Video {} not found in playlist {}",
                video_id,
                playlist_id
            ));
        }

        let playlist_item_id = &playlist_items.items[0].id;
        self.remove_from_playlist_by_item_id(playlist_item_id)
            .await?;

        info!(
            "Successfully removed video {} from playlist {}",
            video_id, playlist_id
        );
        Ok(())
    }

    /// Fetch all videos from the user's channel.
    ///
    /// This method retrieves all videos from the authenticated user's channel
    /// with their details including video ID, title, description, status, dates, etc.
    ///
    /// # Returns
    /// * Result containing a vector of VideoDetails
    ///
    /// # API Endpoint
    /// GET <https://www.googleapis.com/youtube/v3/search?part=snippet&forMine=true&type=video&maxResults=50&pageToken={pageToken}>
    /// GET <https://www.googleapis.com/youtube/v3/videos?part=snippet,status,recordingDetails&id={video_ids}>
    /// Stream all videos from the user's channel page by page (50 per page).
    ///
    /// Each item is one page of full video details. Memory stays O(page)
    /// instead of O(channel), so callers that process videos incrementally
    /// never materialize the whole channel. Fetch errors surface as `Err`
    /// items and terminate the stream.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use anyhow::Result;
    /// # use futures::StreamExt;
    /// # async fn example(client: &rust_yt_uploader::YouTubeClient) -> Result<()> {
    /// let pages = client.video_pages();
    /// futures::pin_mut!(pages);
    /// while let Some(page) = pages.next().await {
    ///     eprintln!("got {} videos", page?.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn video_pages(&self) -> impl Stream<Item = Result<Vec<VideoDetails>>> + '_ {
        stream::unfold(Some(None::<String>), move |state| async move {
            let page_token = state?; // None => stream exhausted
            match self.fetch_video_page(page_token).await {
                Ok((page, next)) if !page.is_empty() => Some((Ok(page), Some(next))),
                Ok(_) => None,                  // empty page = end of channel
                Err(e) => Some((Err(e), None)), // yield the error, then terminate
            }
        })
    }

    /// Fetch one page of channel videos (search + per-video details).
    /// Returns the page and the token for the next page, if any.
    async fn fetch_video_page(
        &self,
        page_token: Option<String>,
    ) -> Result<(Vec<VideoDetails>, Option<String>)> {
        // Build the search endpoint with pagination
        let mut endpoint =
            String::from("search?part=snippet&forMine=true&type=video&maxResults=50");
        if let Some(token) = &page_token {
            endpoint.push_str(&format!("&pageToken={}", token));
        }

        let response = self.client.get(&endpoint).await?.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to list videos with status {}: {}",
                status,
                text
            ));
        }

        let search_response: types::SearchResponse = response.json().await?;

        let video_ids: Vec<String> = search_response
            .items
            .iter()
            .map(|item| item.id.video_id.clone())
            .collect();

        let video_details = self.fetch_video_details(&video_ids).await?;
        Ok((video_details, search_response.next_page_token))
    }

    /// Fetch all videos from the user's channel.
    ///
    /// This method retrieves all videos from the authenticated user's channel
    /// with their details including video ID, title, description, status, dates, etc.
    ///
    /// # Returns
    /// * Result containing a vector of VideoDetails
    ///
    /// # API Endpoint
    /// GET <https://www.googleapis.com/youtube/v3/search?part=snippet&forMine=true&type=video&maxResults=50&pageToken={pageToken}>
    /// GET <https://www.googleapis.com/youtube/v3/videos?part=snippet,status,recordingDetails&id={video_ids}>
    pub async fn list_all_videos(&self) -> Result<Vec<VideoDetails>> {
        info!("Fetching all videos from user's channel");

        let mut all_videos = Vec::new();
        let pages = self.video_pages();
        futures::pin_mut!(pages);
        while let Some(page) = pages.next().await {
            all_videos.extend(page?);
        }

        info!("Successfully fetched {} videos", all_videos.len());
        Ok(all_videos)
    }

    /// Fetch detailed information for a list of video IDs.
    ///
    /// # Arguments
    /// * `video_ids` - Vector of YouTube video IDs
    ///
    /// # Returns
    /// * Result containing a vector of VideoDetails
    async fn fetch_video_details(&self, video_ids: &[String]) -> Result<Vec<VideoDetails>> {
        if video_ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids_string = video_ids.join(",");
        let endpoint = format!(
            "videos?part=snippet,status,recordingDetails,contentDetails&id={}",
            ids_string
        );

        let video_response: types::VideoResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video details")
            .await?;

        let videos = video_response
            .items
            .into_iter()
            .map(|item| VideoDetails {
                id: item.id,
                title: item.snippet.title,
                description: item.snippet.description,
                status: item.status.privacy_status,
                upload_date: item.snippet.published_at,
                category_id: item.snippet.category_id,
                tags: item.snippet.tags.unwrap_or_default(),
                default_language: item.snippet.default_language,
                default_audio_language: item.snippet.default_audio_language,
                recording_date: item.recording_details.and_then(|rd| rd.recording_date),
                duration: item
                    .content_details
                    .as_ref()
                    .and_then(|cd| cd.duration.clone()),
                caption: item.content_details.and_then(|cd| cd.caption),
            })
            .collect();

        Ok(videos)
    }

    /// Update video metadata (snippet and recording details).
    ///
    /// This method updates the specified video's metadata including title, description,
    /// language settings, and recording date.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID to update
    /// * `default_language` - Optional default language code (e.g., "zh", "en")
    /// * `default_audio_language` - Optional default audio language code (e.g., "zh-Hans", "en")
    ///
    /// # Returns
    /// * Result indicating success or failure
    ///
    /// # API Endpoint
    /// PUT <https://www.googleapis.com/youtube/v3/videos?part=snippet,recordingDetails>
    pub async fn update_video_language(
        &self,
        video_id: &str,
        default_language: Option<&str>,
        default_audio_language: Option<&str>,
    ) -> Result<()> {
        info!("Updating video {} with language settings", video_id);

        // First, fetch the current video details
        let endpoint = format!("videos?part=snippet,recordingDetails&id={}", video_id);

        let video_response: types::VideoListResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video details")
            .await?;

        if video_response.items.is_empty() {
            return Err(anyhow!("Video {} not found", video_id));
        }

        let mut video_item = video_response.items[0].clone();

        // Update the snippet part with language settings
        if let Some(snippet) = video_item
            .get_mut("snippet")
            .and_then(|s| s.as_object_mut())
        {
            if let Some(lang) = default_language {
                snippet.insert("defaultLanguage".to_string(), json!(lang));
            }
            if let Some(audio_lang) = default_audio_language {
                snippet.insert("defaultAudioLanguage".to_string(), json!(audio_lang));
            }
        }

        // Remove unnecessary fields for update
        if let Some(obj) = video_item.as_object_mut() {
            obj.remove("kind");
            obj.remove("etag");
        }

        // Send PUT request to update the video
        let response = self
            .client
            .put("videos?part=snippet,recordingDetails")
            .await?
            .json(&video_item)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to update video with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully updated video: {}", video_id);
        Ok(())
    }

    pub async fn update_video_recording_date(
        &self,
        video_id: &str,
        recording_date: &str,
    ) -> Result<()> {
        info!(
            "Updating video {} with recording date: {}",
            video_id, recording_date
        );

        // First, fetch the current video details
        let endpoint = format!("videos?part=snippet,recordingDetails&id={}", video_id);

        let video_response: types::VideoListResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video details")
            .await?;

        if video_response.items.is_empty() {
            return Err(anyhow!("Video {} not found", video_id));
        }

        let mut video_item = video_response.items[0].clone();

        // Update or create the recordingDetails part with the recording date
        if let Some(obj) = video_item.as_object_mut() {
            let recording_details = obj
                .entry("recordingDetails".to_string())
                .or_insert_with(|| json!({}));

            if let Some(details_obj) = recording_details.as_object_mut() {
                details_obj.insert("recordingDate".to_string(), json!(recording_date));
            }
        } else {
            return Err(anyhow!("Failed to parse video item as object"));
        }

        // Remove unnecessary fields for update
        if let Some(obj) = video_item.as_object_mut() {
            obj.remove("kind");
            obj.remove("etag");
        }

        // Send PUT request to update the video
        let response = self
            .client
            .put("videos?part=snippet,recordingDetails")
            .await?
            .json(&video_item)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to update video with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully updated video recording date: {}", video_id);
        Ok(())
    }

    pub async fn update_video_description(
        &self,
        video_id: &str,
        additional_content: &str,
    ) -> Result<()> {
        info!("Appending content to description for video {}", video_id);

        let endpoint = format!("videos?part=snippet&id={}", video_id);

        let video_response: types::VideoListResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video details")
            .await?;

        if video_response.items.is_empty() {
            return Err(anyhow!("Video {} not found", video_id));
        }

        let mut video_item = video_response.items[0].clone();

        if let Some(snippet) = video_item
            .get_mut("snippet")
            .and_then(|s| s.as_object_mut())
        {
            if let Some(desc) = snippet.get_mut("description").and_then(|d| d.as_str()) {
                let new_description = if desc.trim().is_empty() {
                    additional_content.to_string()
                } else {
                    format!("{}\n\n{}", desc, additional_content)
                };
                snippet.insert("description".to_string(), json!(new_description));
            } else {
                snippet.insert("description".to_string(), json!(additional_content));
            }
        }

        if let Some(obj) = video_item.as_object_mut() {
            obj.remove("kind");
            obj.remove("etag");
        }

        let response = self
            .client
            .put("videos?part=snippet")
            .await?
            .json(&video_item)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to update video with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully updated video description: {}", video_id);
        Ok(())
    }

    /// Replace (overwrite) a video's description entirely.
    /// Unlike `update_video_description` (which appends), this sets the
    /// description to exactly `new_description`. Used to repair duplicated /
    /// corrupted descriptions by rebuilding from canonical source files.
    pub async fn set_video_description(&self, video_id: &str, new_description: &str) -> Result<()> {
        info!("Setting (overwriting) description for video {}", video_id);

        let endpoint = format!("videos?part=snippet&id={}", video_id);

        let video_response: types::VideoListResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video details")
            .await?;

        if video_response.items.is_empty() {
            return Err(anyhow!("Video {} not found", video_id));
        }

        let mut video_item = video_response.items[0].clone();

        if let Some(snippet) = video_item
            .get_mut("snippet")
            .and_then(|s| s.as_object_mut())
        {
            snippet.insert("description".to_string(), json!(new_description));
        }

        if let Some(obj) = video_item.as_object_mut() {
            obj.remove("kind");
            obj.remove("etag");
        }

        let response = self
            .client
            .put("videos?part=snippet")
            .await?
            .json(&video_item)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to update video with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully set video description: {}", video_id);
        Ok(())
    }

    /// Get the current description of a video.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID
    ///
    /// # Returns
    /// * Result containing the video description
    ///
    /// # API Endpoint
    /// GET <https://www.googleapis.com/youtube/v3/videos?part=snippet&id={video_id}>
    pub async fn get_video_description(&self, video_id: &str) -> Result<String> {
        info!("Fetching description for video {}", video_id);

        let endpoint = format!("videos?part=snippet&id={}", video_id);

        let video_response: types::VideoResponseSimple = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video description")
            .await?;

        if video_response.items.is_empty() {
            return Err(anyhow!("Video {} not found", video_id));
        }

        Ok(video_response.items[0].snippet.description.clone())
    }

    /// Check if content already exists in the video description.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID
    /// * `content` - The content to check for
    ///
    /// # Returns
    /// * Result containing true if content exists, false otherwise
    pub async fn description_contains(&self, video_id: &str, content: &str) -> Result<bool> {
        let description = self.get_video_description(video_id).await?;
        Ok(description.contains(content))
    }

    pub async fn update_video_tags(
        &self,
        video_id: &str,
        additional_tags: &[String],
    ) -> Result<()> {
        info!(
            "Adding {} tags to video {}",
            additional_tags.len(),
            video_id
        );

        let endpoint = format!("videos?part=snippet&id={}", video_id);

        let video_response: types::VideoListResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch video details")
            .await?;

        if video_response.items.is_empty() {
            return Err(anyhow!("Video {} not found", video_id));
        }

        let mut video_item = video_response.items[0].clone();

        if let Some(snippet) = video_item
            .get_mut("snippet")
            .and_then(|s| s.as_object_mut())
        {
            let existing_tags = snippet
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                        .collect::<std::collections::HashSet<_>>()
                })
                .unwrap_or_default();

            let new_tags: Vec<String> = additional_tags
                .iter()
                .filter(|tag| !existing_tags.contains(&tag.to_lowercase()))
                .map(|tag| tag.to_string())
                .collect();

            if !new_tags.is_empty() {
                let mut all_tags: Vec<String> = snippet
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                all_tags.extend(new_tags);
                snippet.insert("tags".to_string(), json!(all_tags));
            } else {
                info!("No new tags to add (all tags already exist)");
            }
        }

        if let Some(obj) = video_item.as_object_mut() {
            obj.remove("kind");
            obj.remove("etag");
        }

        let response = self
            .client
            .put("videos?part=snippet")
            .await?
            .json(&video_item)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to update video with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully updated video tags: {}", video_id);
        Ok(())
    }

    /// List all available captions/subtitles for a specific video.
    ///
    /// This method retrieves all caption tracks available for a given video,
    /// including auto-generated, manually added, and draft captions.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID
    ///
    /// # Returns
    /// * Result containing a vector of CaptionDetails
    ///
    /// # API Endpoint
    /// GET <https://www.googleapis.com/youtube/v3/captions?videoId={video_id}&part=snippet>
    pub async fn list_video_captions(&self, video_id: &str) -> Result<Vec<CaptionDetails>> {
        info!("Fetching captions for video: {}", video_id);

        let endpoint = format!("captions?part=snippet&videoId={}", video_id);

        let caption_response: types::CaptionResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch captions")
            .await?;

        let captions: Vec<CaptionDetails> = caption_response
            .items
            .into_iter()
            .map(|item| CaptionDetails {
                id: item.id,
                video_id: item.snippet.video_id,
                language: item.snippet.language,
                is_auto_synced: item.snippet.is_auto_synced,
                is_cc: item.snippet.is_cc,
                is_large: item.snippet.is_large,
                is_draft: item.snippet.is_draft,
                is_easy_reader: item.snippet.is_easy_reader,
                audio_track_type: item.snippet.audio_track_type,
                name: item.snippet.name,
            })
            .collect();

        info!("Found {} caption(s) for video {}", captions.len(), video_id);
        Ok(captions)
    }

    /// Delete a caption/subtitle track by caption ID.
    ///
    /// # Arguments
    /// * `caption_id` - The YouTube caption track ID to delete
    ///
    /// # API Endpoint
    /// DELETE <https://www.googleapis.com/youtube/v3/captions?id={caption_id}>
    pub async fn delete_caption(&self, caption_id: &str) -> Result<()> {
        info!("Deleting caption track: {}", caption_id);

        let response = self
            .client
            .delete(&format!("captions?id={}", caption_id))
            .await?
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to delete caption with status {}: {}",
                status,
                text
            ));
        }

        info!("Successfully deleted caption track: {}", caption_id);
        Ok(())
    }

    /// List captions for all videos in the user's channel.
    ///
    /// # Returns
    /// * Result containing a vector of tuples with (VideoId, Vec<CaptionDetails>)
    pub async fn list_all_captions(&self) -> Result<Vec<(String, Vec<CaptionDetails>)>> {
        info!("Fetching all videos and their captions");

        let mut video_captions = Vec::new();
        // Stream pages: only video IDs are needed, so the full VideoDetails
        // (with descriptions) never materializes channel-wide.
        let pages = self.video_pages();
        futures::pin_mut!(pages);
        while let Some(page) = pages.next().await {
            for video in page? {
                match self.list_video_captions(&video.id).await {
                    Ok(captions) => {
                        video_captions.push((video.id, captions));
                    }
                    Err(e) => {
                        warn!("Failed to fetch captions for video {}: {}", video.id, e);
                    }
                }
            }
        }

        Ok(video_captions)
    }

    /// Upload a caption/subtitle file to a video.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID
    /// * `srt_file_path` - Path to the SRT subtitle file
    /// * `language` - Language code (e.g., "en", "zh", "fr")
    /// * `name` - Optional name for the caption track
    ///
    /// # Returns
    /// * Result containing the caption ID
    ///
    /// # API Endpoint
    /// POST <https://www.googleapis.com/upload/youtube/v3/captions?uploadType=multipart&part=snippet>
    pub async fn upload_caption(
        &self,
        video_id: &str,
        srt_file_path: &Path,
        language: &str,
        name: Option<&str>,
    ) -> Result<String> {
        info!(
            "Uploading subtitle file '{}' for video {} (language: {})",
            srt_file_path.display(),
            video_id,
            language
        );

        if !srt_file_path.exists() {
            return Err(anyhow!("SRT file not found: {}", srt_file_path.display()));
        }

        let srt_content = tokio::fs::read_to_string(srt_file_path).await?;

        let metadata_json = json!({
            "snippet": {
                "videoId": video_id,
                "language": language,
                "name": name.unwrap_or(language)
            }
        });

        let form = reqwest::multipart::Form::new()
            .part(
                "snippet",
                reqwest::multipart::Part::text(metadata_json.to_string())
                    .mime_str("application/json")?,
            )
            .part(
                "media",
                reqwest::multipart::Part::text(srt_content)
                    .mime_str("text/plain")?
                    .file_name(
                        srt_file_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    ),
            );

        let upload_url = "https://www.googleapis.com/upload/youtube/v3/captions?uploadType=multipart&part=snippet";

        let response = self
            .client
            .request(reqwest::Method::POST, upload_url)
            .await?
            .multipart(form)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to upload caption with status {}: {}",
                status,
                text
            ));
        }

        let upload_response: types::CaptionUploadResponse = response.json().await?;
        let caption_id = upload_response.id;

        info!("Successfully uploaded subtitle with ID: {}", caption_id);

        Ok(caption_id)
    }

    /// Post a comment to a video with optional pinning.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID
    /// * `comment_text` - The text content of the comment
    ///
    /// # Returns
    /// * Result containing the comment ID
    ///
    /// # API Endpoint
    /// POST <https://www.googleapis.com/youtube/v3/commentThreads?part=snippet>
    pub async fn post_comment(&self, video_id: &str, comment_text: &str) -> Result<String> {
        info!(
            "Posting comment to video {} with text length: {}",
            video_id,
            comment_text.len()
        );

        let comment_json = json!({
            "snippet": {
                "videoId": video_id,
                "topLevelComment": {
                    "snippet": {
                        "textOriginal": comment_text
                    }
                }
            }
        });

        let comment_response: types::CommentResponse = self
            .execute_and_parse(
                self.client
                    .post("commentThreads?part=snippet")
                    .await?
                    .json(&comment_json),
                "post comment",
            )
            .await?;
        let comment_id = comment_response.id;

        info!("Successfully posted comment with ID: {}", comment_id);

        Ok(comment_id)
    }

    /// Pin a comment (make it a featured comment) on a video.
    ///
    /// # Arguments
    /// * `comment_id` - The ID of the comment thread to pin
    ///
    /// # Returns
    /// * Result indicating success or failure
    ///
    /// # API Endpoint
    /// PUT <https://www.googleapis.com/youtube/v3/commentThreads?part=snippet>
    pub async fn pin_comment(&self, comment_id: &str) -> Result<()> {
        info!("Pinning comment: {}", comment_id);

        // YouTube Data API v3 has no dedicated "pin" endpoint. The closest supported
        // operation is commentThreads.update, but it may return 403 even when the
        // comment ends up visually pinned (YouTube auto-features channel-owner comments).
        // We attempt the update and log a warning on failure rather than hard-erroring.

        // Fetch the comment thread first, with retries to handle propagation delay.
        let endpoint = format!("commentThreads?part=snippet&id={}", comment_id);

        let mut retry_count = 0;
        let max_retries = 5;
        let retry_delay_ms = 1000u64;

        let mut comment_item = loop {
            let response = self.client.get(&endpoint).await?.send().await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(anyhow!(
                    "Failed to fetch comment details with status {}: {}",
                    status,
                    text
                ));
            }

            let comment_response: types::CommentThreadResponse = response.json().await?;

            if !comment_response.items.is_empty() {
                break comment_response.items[0].clone();
            }

            retry_count += 1;
            if retry_count >= max_retries {
                return Err(anyhow!(
                    "Comment {} not found after {} retries (propagation delay?)",
                    comment_id,
                    max_retries
                ));
            }

            warn!(
                "Comment {} not found yet, retrying in {}ms ({}/{})...",
                comment_id, retry_delay_ms, retry_count, max_retries
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay_ms)).await;
        };

        // Attempt to set isRepliesDisabled=true via commentThreads.update.
        // This is the only writable field that can influence comment ordering in the API.
        // Note: this may 403 — YouTube auto-pins channel-owner comments regardless.
        if let Some(snippet) = comment_item
            .get_mut("snippet")
            .and_then(|s| s.as_object_mut())
        {
            snippet.insert("isRepliesDisabled".to_string(), json!(true));
        }
        if let Some(obj) = comment_item.as_object_mut() {
            obj.remove("kind");
            obj.remove("etag");
        }

        let response = self
            .client
            .put("commentThreads?part=snippet")
            .await?
            .json(&comment_item)
            .send()
            .await?;

        if response.status().is_success() {
            info!("Successfully updated comment thread for {}", comment_id);
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            // Downgrade to a warning: YouTube auto-pins channel-owner comments, so the
            // visual pin should already be in effect even when the API call fails.
            warn!(
                "commentThreads.update returned {} for comment {} — \
                 the comment may still appear pinned via YouTube's auto-feature. \
                 Details: {}",
                status, comment_id, text
            );
        }

        Ok(())
    }

    /// Check if a comment with the same text already exists on a video.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID
    /// * `comment_text` - The text to search for
    ///
    /// # Returns
    /// * Result containing true if a matching comment exists, false otherwise
    pub async fn comment_exists(&self, video_id: &str, comment_text: &str) -> Result<bool> {
        info!("Checking if comment already exists on video {}", video_id);

        let endpoint = format!(
            "commentThreads?part=snippet&videoId={}&textFormat=plainText",
            video_id
        );

        let comments_response: types::CommentsResponse = self
            .execute_and_parse(self.client.get(&endpoint).await?, "fetch comments")
            .await?;

        // Check if any comment matches the text we're trying to post
        for item in comments_response.items {
            if item.snippet.top_level_comment.snippet.text_original.trim() == comment_text.trim() {
                info!("Found matching comment on video {}", video_id);
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Check if a video already has at least one pinned (featured) comment.
    ///
    /// Note: YouTube's API v3 does NOT directly expose "pinned" status via commentThreads.
    /// This function uses a heuristic: checks if there's a comment from the video owner.
    /// Only the channel owner can pin comments on their own videos.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID to inspect
    ///
    /// # Returns
    /// * `Ok(true)` if at least one comment is from the video owner (likely pinned),
    ///   `Ok(false)` otherwise
    pub async fn has_pinned_comment(&self, video_id: &str) -> Result<bool> {
        info!("Checking for pinned comments on video {}", video_id);

        // Step 1: fetch the video to learn which channel owns it.
        // The JSON field is "channelId" (camelCase) so we must rename explicitly.
        let video_channel_id: Option<String> = {
            let video_response = self
                .client
                .get(&format!("videos?part=snippet&id={}", video_id))
                .await?
                .send()
                .await?;

            if video_response.status().is_success() {
                match video_response.json::<types::VideoResponseChannel>().await {
                    Ok(vr) => {
                        if let Some(item) = vr.items.first() {
                            if let Some(cid) = &item.snippet.channel_id {
                                info!("Video {} belongs to channel {}", video_id, cid);
                                Some(cid.clone())
                            } else {
                                warn!("Channel ID not found in video response");
                                None
                            }
                        } else {
                            warn!("Video {} not found in API response", video_id);
                            None
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse video response: {}", e);
                        None
                    }
                }
            } else {
                let status = video_response.status();
                let text = video_response.text().await.unwrap_or_default();
                warn!(
                    "Failed to fetch video details (status {}): {}",
                    status, text
                );
                None
            }
        };

        // Step 2: fetch comment threads.
        // The correct API shape (matching comment_exists) is:
        //   item.snippet.topLevelComment.snippet.authorChannelId.value
        //   item.snippet.isRepliesDisabled
        let endpoint = format!(
            "commentThreads?part=snippet&videoId={}&textFormat=plainText",
            video_id
        );
        let response = self.client.get(&endpoint).await?.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to fetch comments with status {}: {}",
                status,
                text
            ));
        }

        let comments_response: types::CommentsResponseAuthor = response.json().await?;
        let total_items = comments_response.items.len();
        debug!(
            "Received {} comment threads for video {}",
            total_items, video_id
        );

        // Primary check: any top-level comment from the channel owner → pinned.
        // Only the channel owner can pin comments on their own videos.
        if let Some(channel_id) = &video_channel_id {
            for item in &comments_response.items {
                let author = item
                    .snippet
                    .top_level_comment
                    .snippet
                    .author_channel_id
                    .as_ref()
                    .and_then(|id| id.value.as_deref());
                debug!(
                    "Comment author channel: {:?}, owner channel: {}",
                    author, channel_id
                );
                if author == Some(channel_id.as_str()) {
                    info!(
                        "Found comment from channel owner {} on video {} — treating as pinned",
                        channel_id, video_id
                    );
                    return Ok(true);
                }
            }
        }

        // Fallback: isRepliesDisabled can indicate a featured/pinned comment in some cases.
        if comments_response
            .items
            .iter()
            .any(|item| item.snippet.is_replies_disabled)
        {
            info!(
                "Found comment with isRepliesDisabled=true on video {} — treating as pinned",
                video_id
            );
            return Ok(true);
        }

        debug!(
            "No pinned comment found on video {} ({} comments checked)",
            video_id, total_items
        );
        Ok(false)
    }
}

/// Upload videos using individual schema format (sequential).
pub async fn upload_individual_sequential(
    config: IndividualConfigRoot,
    _show_progress: bool,
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

    let uploader = YouTubeClient::new(profile)
        .await
        .map_err(|err| anyhow!("Failed to initialize YouTubeUploader: {}", err))?;

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
pub async fn upload_batch_sequential(
    config: BatchConfigRoot,
    show_progress: bool,
    profile: &str,
) -> Result<()> {
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

        // Create uploader with progress bar for this video if progress is enabled
        let uploader = if show_progress {
            // Get file size for progress bar
            let file_path_expanded = shellexpand::tilde(&file_to_upload);
            let file_path_obj = Path::new(file_path_expanded.as_ref());
            let metadata = tokio::fs::metadata(&file_path_obj).await?;
            let file_size = metadata.len();

            let progress_reporter = Arc::new(ProgressBarReporter::new(&full_title, file_size));
            YouTubeClient::with_progress_reporter(profile, progress_reporter)
                .await
                .map_err(|err| anyhow!("Failed to initialize YouTubeUploader: {}", err))?
        } else {
            YouTubeClient::new(profile)
                .await
                .map_err(|err| anyhow!("Failed to initialize YouTubeUploader: {}", err))?
        };

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
    _show_progress: bool,
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

    let uploader = YouTubeClient::new(profile)
        .await
        .map_err(|err| anyhow!("Failed to initialize YouTubeUploader: {}", err))?;
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
            let uploader = &uploader;
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
