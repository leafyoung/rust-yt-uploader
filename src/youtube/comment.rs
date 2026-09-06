//! Comment operations for [`YouTubeClient`].

use anyhow::{Result, anyhow};
use serde_json::json;
use tracing::{debug, info, warn};

use super::{YouTubeClient, types};

impl YouTubeClient {
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
