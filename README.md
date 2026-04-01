# Rust YouTube Uploader

A high-performance, memory-safe Rust library for YouTube video uploading with OAuth 2.0 authentication, featuring both programmatic API and command-line interface.

## Features

- **OAuth 2.0 Authentication**: Secure authentication with YouTube API using OAuth 2.0 flow with PKCE support
- **Dual Configuration Formats**: Support for both individual and batch YAML configuration formats
- **Concurrent Uploads**: Async upload mode with configurable concurrency (default: 3)
- **Resumable Uploads**: Robust upload handling with automatic retry and resumption
- **Progress Tracking**: Real-time upload progress bars for sequential uploads
- **MTS File Support**: Special handling for MTS files with correct MIME type
- **Comprehensive Validation**: Input validation for all configuration parameters
- **Retry Logic**: Exponential backoff with jitter for handling transient failures
- **Memory Safety**: Zero-cost abstractions with compile-time safety guarantees

## Installation

### As a Library Dependency

Add to your `Cargo.toml`:

```toml
[dependencies]
rust-yt-uploader = "0.2.8"
```

### As a CLI Tool

#### Prerequisites

- A Google Cloud project with YouTube Data API v3 enabled
- OAuth 2.0 client credentials

1. Go to the [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project or select an existing one
3. Enable the YouTube Data API v3
4. Create OAuth 2.0 credentials (Desktop application)
5. Download the credentials and save as `client_secret-{profile}.json` (e.g., `client_secret-work.json`)
6. Place the file in the working directory

The first time you run the uploader with a profile, it will:

1. Load credentials from `client_secret-{profile}.json`
2. Display an authorization URL
3. Open your browser for authentication
4. Ask you to paste the authorization code
5. Save the tokens to `youtube-oauth2-{profile}.json`

#### Build from Source

```bash
git clone https://github.com/leafyoung/rust-yt-uploader
cd rust-yt-uploader
cargo build --release --bin yt-upload
cargo build --release --bin yt-list
cargo build --release --bin yt-update-lang
```

The binaries will be available after building.

## Usage

### As a Library

```rust
use rust_yt_uploader::{YouTubeClient, ConfigFormat, BatchConfigRoot};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = BatchConfigRoot::from_file("config.yaml")?;

    // Create authenticated client with profile
    // Credentials: client_secret-work.json
    // Token: youtube-oauth2-work.json
    let client = YouTubeClient::new("work").await?;

    // Upload videos
    client.upload_batch(&config).await?;

    Ok(())
}
```

#### Using GoogleOAuth Directly

For more advanced use cases requiring direct API access:

```rust
use rust_yt_uploader::google_oauth::{GoogleOAuth, Credentials};
use rust_yt_uploader::{youtube_client, credentials_path_for_profile, token_path_for_profile};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create OAuth client with profile-based paths
    let profile = "work";
    let oauth_client = GoogleOAuth::new(
        credentials_path_for_profile(profile)?,  // client_secret-work.json
        token_path_for_profile(profile)?,        // youtube-oauth2-work.json
        youtube_client::default_youtube_scopes(),
        youtube_client::build_youtube_base_url(),
    ).await?;

    // Use the authenticated client for custom API calls
    // The client handles token refresh automatically
    let response = oauth_client
        .http_client
        .get("https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true")
        .bearer_auth(&oauth_client.access_token)
        .send()
        .await?;

    println!("API Response: {:?}", response.json::<serde_json::Value>().await?);

    Ok(())
}
```

## YAML Configuration Formats

#### Batch Format (Recommended)

```yaml
common:
  prefix: "My Video Series"
  keywords: "rust,youtube,programming"
  category: ScienceTechnology
  privacyStatus: "private"
  playlistId: "PL1234567890123456"

titles:
  - "Episode 1: Introduction"
  - "Episode 2: Getting Started"

files:
  - "/path/to/video1.mp4"
  - "/path/to/video2.mp4"
  # Each entry can also contain multiple files separated by comma, semicolon, or space:
  # - "/path/to/part1.mp4;/path/to/part2.mp4"
  # - "/path/to/video3.mp4, /path/to/video3_extra.mp4"
```

#### Individual Format

```yaml
videos:
  - title: "My First Video"
    description: "This is my first video"
    keywords: "rust,youtube"
    file: "/path/to/video1.mp4"
    category: ScienceTechnology
    privacyStatus: "private"
    playlistId: "PL1234567890123456"
```

### Configuration Reference

#### Video Categories

| ID  | Category             |
| --- | -------------------- |
| 1   | Film & Animation     |
| 2   | Autos & Vehicles     |
| 10  | Music                |
| 15  | Pets & Animals       |
| 17  | Sports               |
| 20  | Gaming               |
| 22  | People & Blogs       |
| 23  | Comedy               |
| 24  | Entertainment        |
| 25  | News & Politics      |
| 26  | Howto & Style        |
| 27  | Education            |
| 28  | Science & Technology |

#### Privacy Status Options

- `public`: Video is visible to everyone
- `private`: Video is only visible to you
- `unlisted`: Video is visible to anyone with the link

#### Playlist ID Format

Playlist IDs must match the pattern: `^PL[a-zA-Z0-9_-]{16,33}$`

Example: `PL1234567890123456`

### As a CLI Tool

#### yt-upload: Upload Videos

```bash
# Sequential upload with profile (required)
yt-upload --file config.yaml --profile work

# Sequential upload with progress bars
yt-upload --file config.yaml --profile work --progress

# Concurrent upload (3 concurrent by default)
yt-upload --file config.yaml --profile work --async

# Custom concurrency level
yt-upload --file config.yaml --profile work --async --concurrent 5
```

#### yt-list: List and Export Videos

The `yt-list` tool lists all videos from your YouTube channel with comprehensive metadata. This is useful for:

1. **Downloading videos** - Get video IDs for download tools
2. **Updating metadata** - Export video info to update recording date, language, and audio language

**Basic Usage:**

```bash
# List all videos in table format (default) - profile required
yt-list --profile work

# Export as JSON (for programmatic access)
yt-list --profile work --format json

# Export as JSONL (one video per line, useful for piping)
yt-list --profile work --format jsonl

# Save to file instead of stdout
yt-list --profile work --format json --output videos.json

# Show only video IDs (one per line)
yt-list --profile work --ids-only
```

**Output Formats:**

- **table** (default): Human-readable table with video ID, title, status, and recording date
- **json**: Single JSON array containing all videos
- **jsonl**: JSON Lines format - one JSON object per line (great for piping to other tools)

**Video Details Exported:**

Each video includes:

- `id`: YouTube video ID (needed for downloading)
- `title`: Video title
- `description`: Video description
- `status`: Privacy status (public, private, unlisted)
- `upload_date`: When the video was uploaded
- `recording_date`: When the video was recorded (if set)
- `category_id`: YouTube category ID
- `tags`: Video tags/keywords
- `default_language`: Default language for the video
- `default_audio_language`: Default audio language for the video

**Examples:**

```bash
# Export videos to JSON and pipe to jq for further processing
yt-list --profile work --format json | jq '.[].id'

# Extract video IDs and titles for batch download
yt-list --profile work --format jsonl | jq -r '[.id, .title] | join(": ")'

# Save all video metadata for backup
yt-list --profile work --format json --output my_videos_backup.json
```

#### yt-update-lang: Update Language Metadata

The `yt-update-lang` tool automatically updates language metadata for all public videos in your channel. It sets:

- `defaultLanguage`: zh (Chinese)
- `defaultAudioLanguage`: zh-Hans (Simplified Chinese)

For any videos that don't already have these values set.

**Basic Usage:**

```bash
# Show what would be updated (dry run) - profile required
yt-update-lang --profile work --dry-run

# Update all public videos with language metadata
yt-update-lang --profile work

# Verbose mode - show each video being processed
yt-update-lang --profile work --verbose

# Only update videos with no language metadata at all
yt-update-lang --profile work --only-empty
```

**Features:**

- **Dry Run Mode** (`--dry-run`): Preview which videos would be updated without making changes
- **Verbose Output** (`--verbose`): Show detailed information about each video being updated
- **Smart Filtering** (`--only-empty`): Only update videos with completely empty language fields
- **Rate Limiting**: Automatically adds small delays between updates to avoid API rate limits
- **Progress Tracking**: Shows progress and summary of successful/failed updates

**Examples:**

```bash
# Preview changes before applying
yt-update-lang --profile work --dry-run

# Update with verbose output to see what's happening
yt-update-lang --profile work --verbose

# Combine with yt-list to verify your videos first
yt-list --profile work --format json | jq '.[] | {id, title, default_language, default_audio_language}'

# Then update them
yt-update-lang --profile work
```

**Use Cases:**

1. **Batch Language Setup** - Set correct language metadata after bulk uploads
2. **Content Localization** - Ensure Chinese content is properly marked as such
3. **Accessibility** - Help YouTube properly display captions and audio tracks
4. **Content Organization** - Maintain consistent metadata across your channel

### Key Dependencies

- `tokio`: Async runtime for high-performance I/O
- `reqwest`: HTTP client for API calls
- `serde`/`serde_yaml`: Configuration parsing and serialization
- `clap`: Command-line argument parsing
- `validator`: Input validation with custom validators
- `tracing`: Structured logging
- `anyhow`/`thiserror`: Comprehensive error handling

### Security Notes

- Never commit `client_secret-{profile}.json` or `youtube-oauth2-{profile}.json` files to version control
- Store credentials securely with appropriate file permissions (600)
- Regularly rotate OAuth tokens if needed
- Use private/unlisted privacy settings for sensitive content

## Development

### Prerequisites

- Rust 2024 edition or later
- `pre-commit` for git hooks (optional but recommended)

### Setting Up Pre-commit Hooks

Pre-commit hooks automatically run linters, formatters, and tests before each commit:

```bash
# Install pre-commit (if not already installed)
pip install pre-commit

# Install the git hooks
pre-commit install

# Run hooks manually on all files
pre-commit run --all-files

# Update hooks to the latest version
pre-commit autoupdate
```

The pre-commit configuration includes:

- **cargo fmt** - Checks code formatting
- **cargo clippy** - Runs the Rust linter
- **cargo test** - Runs all tests
- **Generic checks** - YAML/JSON syntax, trailing whitespace, merge conflicts, etc.

### Manual Testing

```bash
# Format code
cargo fmt

# Check formatting without making changes
cargo fmt -- --check

# Run linter
cargo clippy --all-targets --all-features

# Run tests
cargo test --all-features

# Run tests with output
cargo test --all-features -- --nocapture

# Run specific test
cargo test test_name
```

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Built with the Tokio async runtime for high-performance I/O
- Use yup_oauth2 as an alternative
