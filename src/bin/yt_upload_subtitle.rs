//! YouTube subtitle uploader CLI
//!
//! This binary uploads SRT subtitle files to YouTube videos.

use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::path::PathBuf;
use tracing::info;

/// YouTube subtitle uploader CLI
#[derive(Parser)]
#[command(name = "yt-upload-subtitle")]
#[command(about = "Upload SRT subtitle file to a YouTube video")]
#[command(long_about = r#"
Upload an SRT subtitle file to a YouTube video.

This tool uploads subtitle files to your YouTube videos, supporting:
- SRT format subtitles
- Custom language codes (e.g., "en", "zh", "fr")
- Optional custom names for caption tracks

The video must be owned by the authenticated user.
"#)]
struct Cli {
    /// YouTube video ID
    #[arg(short, long)]
    video_id: String,

    /// Path to the SRT subtitle file
    #[arg(short, long)]
    srt_file: PathBuf,

    /// Language code for the subtitle (e.g., "en", "zh", "fr")
    #[arg(short, long)]
    language: String,

    /// Optional name for the caption track (defaults to language code)
    #[arg(long)]
    name: Option<String>,

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    info!("Starting YouTube subtitle uploader");

    // Validate profile name
    validate_profile_name(&cli.profile)?;
    info!("Using profile: {}", cli.profile);

    info!("Video ID: {}", cli.video_id);
    info!("SRT file: {}", cli.srt_file.display());
    info!("Language: {}", cli.language);

    let uploader = YouTubeClient::new(&cli.profile).await?;

    let caption_id = uploader
        .upload_caption(
            &cli.video_id,
            &cli.srt_file,
            &cli.language,
            cli.name.as_deref(),
        )
        .await?;

    println!("Subtitle uploaded successfully!");
    println!("Caption ID: {}", caption_id);

    info!("Upload complete");
    Ok(())
}
