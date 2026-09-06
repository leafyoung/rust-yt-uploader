//! Video metadata operations for [`YouTubeClient`].

use anyhow::{Result, anyhow};
use futures::StreamExt;
use serde_json::json;
use tracing::info;

use super::{VideoDetails, YouTubeClient, types};

impl YouTubeClient {
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
}
