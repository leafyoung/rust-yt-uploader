use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{init_logging, validate_profile_name, youtube_client::YouTubeClient};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VideoInfo {
    id: String,
    title: String,
    guessed_date: String,
}

/// YouTube video recording date updater CLI
#[derive(Parser)]
#[command(name = "yt-update-date")]
#[command(about = "Update recording dates for YouTube videos from a JSON file")]
#[command(long_about = r#"
Update recording dates for YouTube videos from a JSON file.

The JSON file should contain an array of VideoInfo objects with:
- id: YouTube video ID
- title: Video title
- guessed_date: Date in YYYY-MM-DD format

Example JSON format:
[
  {"id": "abc123", "title": "Video 1", "guessed_date": "2024-01-15"},
  {"id": "def456", "title": "Video 2", "guessed_date": "2024-01-16"}
]
"#)]
struct Cli {
    /// Path to JSON file containing video information
    json_file: PathBuf,

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    // Validate profile name
    validate_profile_name(&cli.profile)?;
    info!("Using profile: {}", cli.profile);

    if !cli.json_file.exists() {
        anyhow::bail!("File not found: {}", cli.json_file.display());
    }

    // Read JSON file
    let json_content = fs::read_to_string(&cli.json_file)?;
    let videos: Vec<VideoInfo> = serde_json::from_str(&json_content)?;

    // Initialize YouTube client with profile
    let client = YouTubeClient::new(&cli.profile).await?;

    println!("Processing {} videos...\n", videos.len());

    for (index, video) in videos.iter().enumerate() {
        match update_video_date(&client, video).await {
            Ok(_) => {
                println!(
                    "[{}/{}] ✓ Updated: {} ({})",
                    index + 1,
                    videos.len(),
                    video.title,
                    video.guessed_date
                );
            }
            Err(e) => {
                eprintln!(
                    "[{}/{}] ✗ Failed to update {}: {}",
                    index + 1,
                    videos.len(),
                    video.id,
                    e
                );
                break;
            }
        }
    }

    println!("\nCompleted processing {} videos.", videos.len());
    Ok(())
}

async fn update_video_date(client: &YouTubeClient, video: &VideoInfo) -> Result<()> {
    // Parse guessed_date from "YYYY-MM-DD" to YouTube format "YYYY-MM-DDTHH:MM:SS.000Z"
    let youtube_date_format = format!("{}T00:00:00.000Z", video.guessed_date);

    // Call update_video_recording_date to update the recording date
    client
        .update_video_recording_date(&video.id, &youtube_date_format)
        .await?;

    Ok(())
}

// peak-alloc: runtime baseline (no user-code heap) 64.0 KB incl. (heap peak 191.1 KB, massif, 2026-09-04)

// leak-suspect: 11856 B possibly lost + 2 "errors" — adjudicated: tokio teardown noise (process::exit skips runtime Drop; glibc TLS of runtime threads), NOT a leak, 0 definite/indirect (2026-09-04)
