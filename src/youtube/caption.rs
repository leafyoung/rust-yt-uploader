//! Caption/subtitle operations for [`YouTubeClient`].

use anyhow::{Result, anyhow};
use futures::StreamExt;
use serde_json::json;
use std::path::Path;
use tracing::{info, warn};

use super::{CaptionDetails, YouTubeClient, types};

impl YouTubeClient {
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
}
