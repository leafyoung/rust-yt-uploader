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
- OAuth 2.0 client credentials (`client_secret.json`)

1. Go to the [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project or select an existing one
3. Enable the YouTube Data API v3
4. Create OAuth 2.0 credentials (Desktop application)
5. Download the credentials as `client_secret.json`
6. Place the file in the parent directory of the Rust project

The first time you run the uploader, it will:

1. Display an authorization URL
2. Open your browser for authentication
3. Ask you to paste the authorization code
4. Save the tokens to `youtube-oauth2.json`

#### Build from Source

```bash
git clone https://github.com/leafyoung/rust-yt-uploader
cd rust-yt-uploader
cargo build --release --bin yt-upload
```

The binary will be available at `target/release/rust-yt-upload`.

## Usage

### As a Library

```rust
use rust_yt_uploader::{YouTubeClient, ConfigFormat, BatchConfigRoot};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = BatchConfigRoot::from_file("config.yaml")?;

    // Create authenticated client
    let client = YouTubeClient::new(
        "client_secret.json",
        "youtube-oauth2.json"
    ).await?;

    // Upload videos
    client.upload_batch(&config).await?;

    Ok(())
}
```

#### Using GoogleOAuth Directly

For more advanced use cases requiring direct API access:

```rust
use rust_yt_uploader::google_oauth::{GoogleOAuth, Credentials};
use rust_yt_uploader::youtube_client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create OAuth client with custom scopes
    let oauth_client = GoogleOAuth::new(
        "client_secret.json",
        "youtube-oauth2.json",
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

```bash
# Sequential upload (default)
cargo run --bin yt-upload --file config.yaml

# Sequential upload with progress bars
cargo run --bin yt-upload --file config.yaml --progress

# Concurrent upload (3 concurrent by default)
cargo run --bin yt-upload --file config.yaml --async

# Custom concurrency level
cargo run --bin yt-upload --file config.yaml --async --concurrent 5
```

## Performance

### Key Dependencies

- `tokio`: Async runtime for high-performance I/O
- `reqwest`: HTTP client for API calls
- `serde`/`serde_yaml`: Configuration parsing and serialization
- `clap`: Command-line argument parsing
- `validator`: Input validation with custom validators
- `tracing`: Structured logging
- `anyhow`/`thiserror`: Comprehensive error handling

### Security Notes

- Never commit `client_secret.json` or token files to version control
- Store credentials securely with appropriate file permissions (600)
- Regularly rotate OAuth tokens if needed
- Use private/unlisted privacy settings for sensitive content

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Built with the Tokio async runtime for high-performance I/O
- Use yup_oauth2 as an alternative
