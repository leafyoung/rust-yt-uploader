//! YouTube API response types.
//!
//! This module contains all YouTube API response structs, extracted from the
//! former youtube_client.rs monolith
//! to provide a single source of truth and eliminate duplication.

use serde::Deserialize;

// ============================================================================
// Video Upload Response Types
// ============================================================================

/// YouTube API response for video upload
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct VideoUploadResponse {
    pub id: String,
    pub snippet: VideoSnippet,
}

/// Video snippet information from YouTube API
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct VideoSnippet {
    pub title: String,
    pub description: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
}

// ============================================================================
// Playlist Response Types
// ============================================================================

/// Playlist info response from YouTube API
#[derive(Debug, Deserialize)]
pub struct PlaylistInfoResponse {
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
}

/// Page info for pagination
#[derive(Debug, Deserialize)]
pub struct PageInfo {
    #[serde(rename = "totalResults")]
    pub total_results: u32,
}

/// Playlist item response from YouTube API
#[derive(Debug, Deserialize)]
pub struct PlaylistItemResponse {
    pub snippet: PlaylistItemSnippet,
    pub id: String,
}

/// Playlist item snippet information
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct PlaylistItemSnippet {
    pub position: u32,
    #[serde(rename = "playlistId")]
    pub playlist_id: String,
}

/// Playlist items list response
#[derive(Debug, Deserialize)]
pub struct PlaylistItemsResponse {
    pub items: Vec<PlaylistItem>,
}

/// Playlist item in a list
#[derive(Debug, Deserialize)]
pub struct PlaylistItem {
    pub id: String,
}

// ============================================================================
// Search Response Types
// ============================================================================

/// Search response from YouTube API
#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub items: Vec<SearchItem>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

/// Search result item
#[derive(Debug, Deserialize)]
pub struct SearchItem {
    pub id: VideoId,
}

/// Video ID from search results
#[derive(Debug, Deserialize)]
pub struct VideoId {
    #[serde(rename = "videoId")]
    pub video_id: String,
}

// ============================================================================
// Video List/Details Response Types
// ============================================================================

/// Generic video list response used across multiple endpoints
#[derive(Debug, Deserialize)]
pub struct VideoListResponse {
    pub items: Vec<serde_json::Value>,
}

/// Video response with full details
#[derive(Debug, Deserialize)]
pub struct VideoResponse {
    pub items: Vec<VideoItem>,
}

/// Video item with complete metadata
#[derive(Debug, Deserialize)]
pub struct VideoItem {
    pub id: String,
    pub snippet: VideoSnippetFull,
    pub status: VideoStatus,
    #[serde(rename = "recordingDetails")]
    pub recording_details: Option<RecordingDetails>,
    #[serde(rename = "contentDetails")]
    pub content_details: Option<ContentDetails>,
}

/// Complete video snippet information
#[derive(Debug, Deserialize)]
pub struct VideoSnippetFull {
    pub title: String,
    pub description: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    #[serde(rename = "publishedAt")]
    pub published_at: String,
    pub tags: Option<Vec<String>>,
    #[serde(rename = "defaultLanguage")]
    pub default_language: Option<String>,
    #[serde(rename = "defaultAudioLanguage")]
    pub default_audio_language: Option<String>,
    #[serde(rename = "channelId")]
    pub channel_id: Option<String>,
}

/// Simple video snippet for basic operations
#[derive(Debug, Deserialize)]
pub struct VideoSnippetSimple {
    pub description: String,
}

/// Video snippet with channel ID for owner detection
#[derive(Debug, Deserialize)]
pub struct VideoSnippetChannel {
    #[serde(rename = "channelId")]
    pub channel_id: Option<String>,
}

/// Video item with simple snippet
#[derive(Debug, Deserialize)]
pub struct VideoItemSimple {
    pub id: String,
    pub snippet: VideoSnippetSimple,
}

/// Video item with channel snippet
#[derive(Debug, Deserialize)]
pub struct VideoItemChannel {
    pub snippet: VideoSnippetChannel,
}

/// Video response with simple items
#[derive(Debug, Deserialize)]
pub struct VideoResponseSimple {
    pub items: Vec<VideoItemSimple>,
}

/// Video response with channel items
#[derive(Debug, Deserialize)]
pub struct VideoResponseChannel {
    pub items: Vec<VideoItemChannel>,
}

/// Video status information
#[derive(Debug, Deserialize)]
pub struct VideoStatus {
    #[serde(rename = "privacyStatus")]
    pub privacy_status: String,
}

/// Recording details for a video
#[derive(Debug, Deserialize)]
pub struct RecordingDetails {
    #[serde(rename = "recordingDate")]
    pub recording_date: Option<String>,
}

/// Content details for a video
#[derive(Debug, Deserialize)]
pub struct ContentDetails {
    pub duration: Option<String>,
    pub caption: Option<String>,
}

// ============================================================================
// Caption Response Types
// ============================================================================

/// Caption list response
#[derive(Debug, Deserialize)]
pub struct CaptionResponse {
    pub items: Vec<CaptionItem>,
}

/// Caption item
#[derive(Debug, Deserialize)]
pub struct CaptionItem {
    pub id: String,
    pub snippet: CaptionSnippet,
}

/// Caption snippet information
#[derive(Debug, Deserialize)]
pub struct CaptionSnippet {
    #[serde(rename = "videoId")]
    pub video_id: String,
    pub language: String,
    #[allow(dead_code)]
    #[serde(rename = "trackKind")]
    pub track_kind: Option<String>,
    #[serde(rename = "isAutoSynced")]
    pub is_auto_synced: Option<bool>,
    #[serde(rename = "isCC")]
    pub is_cc: Option<bool>,
    #[serde(rename = "isLarge")]
    pub is_large: Option<bool>,
    #[serde(rename = "isDraft")]
    pub is_draft: Option<bool>,
    #[serde(rename = "isEasyReader")]
    pub is_easy_reader: Option<bool>,
    #[serde(rename = "audioTrackType")]
    pub audio_track_type: Option<String>,
    pub name: Option<String>,
}

/// Caption upload response
#[derive(Debug, Deserialize)]
pub struct CaptionUploadResponse {
    pub id: String,
}

// ============================================================================
// Comment Response Types
// ============================================================================

/// Comment response with ID
#[derive(Debug, Deserialize)]
pub struct CommentResponse {
    pub id: String,
}

/// Comment thread response
#[derive(Debug, Deserialize)]
pub struct CommentThreadResponse {
    pub items: Vec<serde_json::Value>,
}

/// Comments list response for comment_exists
#[derive(Debug, Deserialize)]
pub struct CommentsResponse {
    #[serde(default)]
    pub items: Vec<CommentItem>,
}

/// Comment item for comment_exists
#[derive(Debug, Deserialize)]
pub struct CommentItem {
    pub snippet: CommentSnippet,
}

/// Comment snippet for comment_exists
#[derive(Debug, Deserialize)]
pub struct CommentSnippet {
    #[serde(rename = "topLevelComment")]
    pub top_level_comment: TopLevelComment,
}

/// Top-level comment for comment_exists
#[derive(Debug, Deserialize)]
pub struct TopLevelComment {
    pub snippet: TextSnippet,
}

/// Text snippet of a comment
#[derive(Debug, Deserialize)]
pub struct TextSnippet {
    #[serde(rename = "textOriginal")]
    pub text_original: String,
}

/// Comments list response for has_pinned_comment
#[derive(Debug, Deserialize)]
pub struct CommentsResponseAuthor {
    #[serde(default)]
    pub items: Vec<CommentItemAuthor>,
}

/// Comment item with author info for has_pinned_comment
#[derive(Debug, Deserialize)]
pub struct CommentItemAuthor {
    pub snippet: ThreadSnippetAuthor,
}

/// Thread snippet with author info for has_pinned_comment
#[derive(Debug, Deserialize)]
pub struct ThreadSnippetAuthor {
    #[serde(rename = "isRepliesDisabled", default)]
    pub is_replies_disabled: bool,
    #[serde(rename = "topLevelComment")]
    pub top_level_comment: TopLevelCommentAuthor,
}

/// Top-level comment with author info for has_pinned_comment
#[derive(Debug, Deserialize)]
pub struct TopLevelCommentAuthor {
    pub snippet: CommentSnippetAuthor,
}

/// Comment snippet with author channel for has_pinned_comment
#[derive(Debug, Deserialize)]
pub struct CommentSnippetAuthor {
    #[serde(rename = "authorChannelId", default)]
    pub author_channel_id: Option<AuthorChannelId>,
}

/// Author channel ID wrapper
#[derive(Debug, Deserialize, Default)]
pub struct AuthorChannelId {
    #[serde(default)]
    pub value: Option<String>,
}

/// Thread snippet with reply settings
#[derive(Debug, Deserialize)]
pub struct ThreadSnippet {
    #[serde(rename = "isRepliesDisabled", default)]
    pub is_replies_disabled: bool,
    #[serde(rename = "topLevelComment")]
    pub top_level_comment: TopLevelComment,
}
