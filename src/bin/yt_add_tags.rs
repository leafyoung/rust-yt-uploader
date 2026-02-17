use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::YouTubeClient;
use std::fs;
use std::path::Path;

/// YouTube video tags updater CLI
#[derive(Parser)]
#[command(name = "yt-add-tags")]
#[command(about = "Append tags to YouTube videos from a text file")]
#[command(long_about = r#"
Append tags to YouTube videos from a comma/semicolon-separated text file.

This tool reads tags from a .txt file and appends them to the existing
tags of specified YouTube videos. Duplicate tags are automatically
filtered out (case-insensitive comparison).

Supports multiple separators: comma (,), Chinese comma (，), semicolon (;), Chinese semicolon (；).

Usage examples:
  yt-add-tags <video_id> <tags_file.txt>
  yt-add-tags abc123 my_tags.txt

Tags file format (any separator):
  tag1, tag2, tag3, another tag
  tag1， tag2， tag3， another tag
  tag1; tag2; tag3; another tag
  tag1； tag2； tag3； another tag
"#)]
struct Cli {
    /// YouTube video ID to update
    video_id: String,

    /// Path to text file containing tags (separated by comma or semicolon)
    tags_file: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let tags_path = Path::new(&cli.tags_file);
    if !tags_path.exists() {
        anyhow::bail!("Tags file not found: {}", cli.tags_file);
    }

    let tags_content = fs::read_to_string(&cli.tags_file)?;

    let tags: Vec<String> = tags_content
        .replace('，', ",")
        .replace('；', ";")
        .split(|c| [',', ';'].contains(&c))
        .map(|tag| tag.to_string())
        .filter(|tag| !tag.trim().is_empty())
        .collect();

    if tags.is_empty() {
        anyhow::bail!("No valid tags found in file: {}", cli.tags_file);
    }

    println!("Reading tags from: {}", cli.tags_file);
    println!("Tags to add: {}", tags.join(";"));
    println!("Updating video: {}", cli.video_id);
    println!();

    let client = YouTubeClient::new().await?;

    client.update_video_tags(&cli.video_id, &tags).await?;

    println!("✓ Successfully updated tags for video {}", cli.video_id);

    Ok(())
}
