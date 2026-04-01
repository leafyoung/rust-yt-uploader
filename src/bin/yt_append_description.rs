use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::fs;
use std::path::Path;
use tracing::info;

/// YouTube video description appender CLI
#[derive(Parser)]
#[command(name = "yt-append-description")]
#[command(about = "Append content to video descriptions from a text file")]
#[command(long_about = r#"
Append content to YouTube video descriptions from a text file.

This tool reads content from a .txt file and appends it to the description
of specified YouTube videos. The existing description and new content are
separated by a blank line (two newlines).

Usage examples:
  yt-append-description <video_id> <content_file.txt>
  yt-append-description abc123 my_content.txt
"#)]
struct Cli {
    /// YouTube video ID to update
    video_id: String,

    /// Path to text file containing content to append
    content_file: String,

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

    let content_path = Path::new(&cli.content_file);
    if !content_path.exists() {
        anyhow::bail!("Content file not found: {}", cli.content_file);
    }

    let additional_content = fs::read_to_string(&cli.content_file)?;

    let additional_content: String = additional_content
        .lines()
        .filter(|line| !line.contains("-- end of file --"))
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    let additional_content = additional_content.trim();

    if additional_content.is_empty() {
        anyhow::bail!("Content file is empty: {}", cli.content_file);
    }

    println!("Reading content from: {}", cli.content_file);
    println!("Updating video: {}", cli.video_id);
    println!();
    println!("Content to append:");
    println!("─────────────────────────────────────────");
    println!("{}", additional_content);
    println!("─────────────────────────────────────────");
    println!();

    let client = YouTubeClient::new(&cli.profile).await?;

    // Check if content already exists in the description
    println!("Checking for duplicate content in video description...");
    let already_exists = client
        .description_contains(&cli.video_id, additional_content)
        .await?;

    if already_exists {
        println!();
        println!("⚠ WARNING: Content already exists in video description!");
        println!("Skipping update to prevent duplicate content.");
        return Ok(());
    }

    println!("No duplicate found. Proceeding with update...");
    println!();

    client
        .update_video_description(&cli.video_id, additional_content)
        .await?;

    println!(
        "✓ Successfully updated description for video {}",
        cli.video_id
    );

    Ok(())
}
