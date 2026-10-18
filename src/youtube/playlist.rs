//! Playlist operations for [`YouTubeClient`].

use anyhow::{Result, anyhow};
use serde_json::json;
use tracing::info;

use super::{YouTubeClient, types};

impl YouTubeClient {
    /// Add a video to a playlist (appended at the end).
    ///
    /// Returns the new playlist item ID.
    ///
    /// # Arguments
    /// * `video_id` - The YouTube video ID to add
    /// * `playlist_id` - The playlist ID to add the video to
    pub async fn add_to_playlist(&self, video_id: &str, playlist_id: &str) -> Result<String> {
        // Omit "position": the API appends to the end of the playlist. Computing it
        // from pageInfo.totalResults overcounts (private/deleted videos) and gets
        // rejected with 400 INVALID_ARGUMENT once past the real end.
        let playlist_item = json!({
            "snippet": {
                "playlistId": playlist_id,
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

    /// Check whether a video is already in a playlist.
    ///
    /// # Arguments
    /// * `playlist_id` - The playlist ID
    /// * `video_id` - The YouTube video ID to look for
    pub async fn is_video_in_playlist(&self, playlist_id: &str, video_id: &str) -> Result<bool> {
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

        Ok(!playlist_items.items.is_empty())
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
}
