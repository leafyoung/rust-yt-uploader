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
- Optional overwrite mode for existing matching caption tracks

By default, the tool checks for an existing caption track with the same language
and name before uploading, and exits with an error if one exists. Use
--overwrite to delete the existing matching track before uploading.

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

    /// Delete an existing caption track with the same language and name before uploading
    #[arg(long)]
    overwrite: bool,

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
    let caption_name = cli.name.as_deref().unwrap_or(&cli.language);

    let existing_captions = uploader.list_video_captions(&cli.video_id).await?;
    let matching_captions: Vec<_> = existing_captions
        .into_iter()
        .filter(|caption| {
            caption.language == cli.language
                && caption.name.as_deref().unwrap_or("") == caption_name
        })
        .collect();

    if !matching_captions.is_empty() && !cli.overwrite {
        anyhow::bail!(
            "Subtitle already exists for video {} with language '{}' and name '{}'. Use --overwrite to delete the existing matching track before uploading.",
            cli.video_id,
            cli.language,
            caption_name
        );
    }

    if cli.overwrite {
        for caption in &matching_captions {
            println!(
                "Deleting existing subtitle track: id={}, language={}, name={}",
                caption.id,
                caption.language,
                caption.name.as_deref().unwrap_or("")
            );
            uploader.delete_caption(&caption.id).await?;
        }
    }

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
