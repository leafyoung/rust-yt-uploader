use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::info;

/// YouTube video tags updater CLI
#[derive(Parser)]
#[command(name = "yt-add-tags")]
#[command(about = "Append tags to YouTube videos from a text file")]
#[command(long_about = r#"
Append tags to YouTube videos from a comma/semicolon-separated text file.

This tool reads tags from a .txt file and appends them to the existing
tags of specified YouTube videos. Duplicate tags are automatically
filtered out (case-insensitive comparison).

Supports multiple video IDs for parallel processing - up to 5 concurrent
updates for ~60% performance improvement on batch operations vs sequential.

Supports multiple separators: comma (,), Chinese comma (，), semicolon (;), Chinese semicolon (；).

Usage examples:
  yt-add-tags -p <profile> <video_id> <tags_file.txt>
  yt-add-tags -p dongli abc123 my_tags.txt
  yt-add-tags -p dongli abc123 def456 ghi789 my_tags.txt

Tags file format (any separator):
  tag1, tag2, tag3, another tag
  tag1， tag2， tag3， another tag
"#)]
struct Cli {
    /// YouTube video ID(s) and tags file path (last argument)
    #[arg(required = true)]
    args: Vec<String>,

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,

    /// Maximum number of concurrent updates (default: 5)
    #[arg(short, long, default_value = "5")]
    concurrent: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    // Validate profile name
    validate_profile_name(&cli.profile)?;
    info!("Using profile: {}", cli.profile);

    // Need at least 2 args: video_id and tags_file
    if cli.args.len() < 2 {
        anyhow::bail!(
            "Usage: yt-add-tags [OPTIONS] -p <PROFILE> <video_id> [<video_id>...] <tags_file.txt>"
        );
    }

    // Last argument is the tags file
    let tags_file = cli.args.last().unwrap().clone();
    let video_ids: Vec<String> = cli.args[..cli.args.len() - 1].to_vec();

    let tags_path = Path::new(&tags_file);
    if !tags_path.exists() {
        anyhow::bail!("Tags file not found: {}", tags_file);
    }

    // Parse tags
    let tags_content = fs::read_to_string(&tags_file)?;
    let tags: Vec<String> = tags_content
        .replace('，', ",")
        .replace('；', ";")
        .split(|c| [',', ';'].contains(&c))
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();

    if tags.is_empty() {
        anyhow::bail!("No valid tags found in file: {}", tags_file);
    }

    println!("Reading tags from: {}", tags_file);
    println!("Tags to add: {}", tags.join(";"));
    println!(
        "Processing {} video(s): {}",
        video_ids.len(),
        video_ids.join(", ")
    );
    println!();

    // Create shared client and semaphore for concurrency
    let client = Arc::new(YouTubeClient::new(&cli.profile).await?);
    let semaphore = Arc::new(Semaphore::new(cli.concurrent));
    let tags = Arc::new(tags);

    let start = std::time::Instant::now();

    // Process videos concurrently
    let mut tasks = Vec::new();
    for video_id in video_ids.clone() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let tags = Arc::clone(&tags);

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await?;
            client.update_video_tags(&video_id, &tags).await
        });
        tasks.push(task);
    }

    // Wait for all tasks and collect results
    let results: Vec<_> = futures::future::join_all(tasks).await;

    let duration = start.elapsed();
    let mut success_count = 0;
    let mut error_count = 0;

    for result in results {
        match result {
            Ok(Ok(())) => success_count += 1,
            Ok(Err(e)) => {
                eprintln!("✗ Error: {}", e);
                error_count += 1;
            }
            Err(e) => {
                eprintln!("✗ Task error: {}", e);
                error_count += 1;
            }
        }
    }

    println!();
    println!("✓ Successfully updated {} video(s)", success_count);
    if error_count > 0 {
        println!("✗ Failed: {} video(s)", error_count);
    }
    println!("  Total time: {:.2}s", duration.as_secs_f64());
    println!(
        "  Average per video: {:.2}s",
        duration.as_secs_f64() / (success_count + error_count) as f64
    );

    Ok(())
}
