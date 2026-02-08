use rust_yt_uploader::youtube_client::YouTubeClient;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VideoInfo {
    id: String,
    title: String,
    guessed_date: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get JSON file path from command line argument
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <json_file_path>", args[0]);
        std::process::exit(1);
    }

    let json_file = &args[1];
    if !Path::new(json_file).exists() {
        eprintln!("Error: File not found: {}", json_file);
        std::process::exit(1);
    }

    // Read JSON file
    let json_content = fs::read_to_string(json_file)?;
    let videos: Vec<VideoInfo> = serde_json::from_str(&json_content)?;

    // Initialize YouTube client
    let client = YouTubeClient::new().await?;

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

async fn update_video_date(
    client: &YouTubeClient,
    video: &VideoInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    // Parse guessed_date from "YYYY-MM-DD" to YouTube format "YYYY-MM-DDTHH:MM:SS.000Z"
    let youtube_date_format = format!("{}T00:00:00.000Z", video.guessed_date);

    // Call update_video_recording_date to update the recording date
    client
        .update_video_recording_date(&video.id, &youtube_date_format)
        .await?;

    Ok(())
}
