//! YouTube subtitle uploader CLI
//!
//! This binary uploads SRT subtitle files to YouTube videos.

use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::YouTubeClient;
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
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let cli = Cli::parse();

    info!("Starting YouTube subtitle uploader");
    info!("Video ID: {}", cli.video_id);
    info!("SRT file: {}", cli.srt_file.display());
    info!("Language: {}", cli.language);

    let uploader = YouTubeClient::new().await?;

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
