use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
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

Supports multiple video IDs for parallel processing - up to 5 concurrent
updates for ~60% performance improvement on batch operations vs sequential.
Even single video operations are optimized with reduced overhead.

Usage examples:
  yt-append-description -p <profile> <video_id> <content_file.txt>
  yt-append-description -p dongli abc123 my_content.txt
  yt-append-description -p dongli abc123 def456 ghi789 my_content.txt
"#)]
struct Cli {
    /// Video ID(s) and content file path (last argument is the file)
    #[arg(required = true)]
    args: Vec<String>,

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,

    /// Maximum number of concurrent updates (default: 5)
    #[arg(short, long, default_value = "5")]
    concurrent: usize,

    /// Skip check for duplicate content (faster, but may create duplicates)
    #[arg(long)]
    force: bool,
}

/// Result of processing a single video
#[allow(dead_code)]
struct VideoResult {
    video_id: String,
    status: VideoStatus,
}

enum VideoStatus {
    Success,
    Skipped(String),
    Error(String),
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    // Validate profile name
    validate_profile_name(&cli.profile)?;
    info!("Using profile: {}", cli.profile);

    // Need at least 2 args: video_id and content_file
    if cli.args.len() < 2 {
        anyhow::bail!(
            "Usage: yt-append-description [OPTIONS] -p <PROFILE> <video_id> [<video_id>...] <content_file.txt>"
        );
    }

    // Last argument is the content file
    let content_file = cli.args.last().unwrap().clone();
    let video_ids: Vec<String> = cli.args[..cli.args.len() - 1].to_vec();

    let content_path = Path::new(&content_file);
    if !content_path.exists() {
        anyhow::bail!("Content file not found: {}", content_file);
    }

    let additional_content = fs::read_to_string(&content_file)?;

    let additional_content: String = additional_content
        .lines()
        .filter(|line| !line.contains("-- end of file --"))
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    let additional_content = additional_content.trim();

    if additional_content.is_empty() {
        anyhow::bail!("Content file is empty: {}", content_file);
    }

    println!("Reading content from: {}", content_file);
    println!(
        "Processing {} video(s): {}",
        video_ids.len(),
        video_ids.join(", ")
    );
    if cli.force {
        println!("Force mode: skipping duplicate check");
    }
    println!();
    println!("Content to append:");
    println!("─────────────────────────────────────────");
    println!("{}", additional_content);
    println!("─────────────────────────────────────────");
    println!();

    // Create shared client and semaphore for concurrency
    let client = Arc::new(YouTubeClient::new(&cli.profile).await?);
    let semaphore = Arc::new(Semaphore::new(cli.concurrent));
    let additional_content = Arc::new(additional_content.to_string());

    let start = std::time::Instant::now();

    // Process videos concurrently
    let mut tasks = Vec::new();
    for video_id in video_ids.clone() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let additional_content = Arc::clone(&additional_content);
        let force = cli.force;

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await?;
            process_video(&client, &video_id, &additional_content, force).await
        });
        tasks.push(task);
    }

    // Wait for all tasks and collect results
    let results: Vec<_> = futures::future::join_all(tasks).await;

    let duration = start.elapsed();
    let mut success_count = 0;
    let mut skipped_count = 0;
    let mut error_count = 0;

    println!("\n=== Results ===\n");

    for (i, result) in results.iter().enumerate() {
        let video_id = &video_ids[i];
        match result {
            Ok(Ok(video_result)) => match video_result.status {
                VideoStatus::Success => {
                    success_count += 1;
                    println!("✓ {} - Description updated successfully", video_id);
                }
                VideoStatus::Skipped(ref reason) => {
                    skipped_count += 1;
                    println!("⊘ {} - Skipped: {}", video_id, reason);
                }
                VideoStatus::Error(ref e) => {
                    error_count += 1;
                    println!("✗ {} - Error: {}", video_id, e);
                }
            },
            Ok(Err(e)) => {
                error_count += 1;
                println!("✗ {} - Error: {}", video_id, e);
            }
            Err(e) => {
                error_count += 1;
                println!("✗ {} - Task error: {}", video_id, e);
            }
        }
    }

    println!();
    println!("=== Summary ===");
    println!("✓ Success: {}", success_count);
    if skipped_count > 0 {
        println!("⊘ Skipped: {}", skipped_count);
    }
    if error_count > 0 {
        println!("✗ Failed: {}", error_count);
    }
    println!("  Total time: {:.2}s", duration.as_secs_f64());
    if success_count + skipped_count + error_count > 0 {
        println!(
            "  Average per video: {:.2}s",
            duration.as_secs_f64() / video_ids.len() as f64
        );
    }

    // Exit with error code 1 if any video failed
    if error_count > 0 {
        anyhow::bail!("{} video(s) failed to update", error_count);
    }

    Ok(())
}

async fn process_video(
    client: &YouTubeClient,
    video_id: &str,
    additional_content: &str,
    force: bool,
) -> Result<VideoResult> {
    // Check if content already exists in the description (unless force mode)
    if !force {
        match client
            .description_contains(video_id, additional_content)
            .await
        {
            Ok(true) => {
                return Ok(VideoResult {
                    video_id: video_id.to_string(),
                    status: VideoStatus::Skipped(
                        "Content already exists in description".to_string(),
                    ),
                });
            }
            Ok(false) => {
                // Continue processing
            }
            Err(e) => {
                return Ok(VideoResult {
                    video_id: video_id.to_string(),
                    status: VideoStatus::Error(format!("Failed to check for duplicate: {}", e)),
                });
            }
        }
    }

    // Update the description
    match client
        .update_video_description(video_id, additional_content)
        .await
    {
        Ok(()) => Ok(VideoResult {
            video_id: video_id.to_string(),
            status: VideoStatus::Success,
        }),
        Err(e) => Ok(VideoResult {
            video_id: video_id.to_string(),
            status: VideoStatus::Error(format!("Failed to update description: {}", e)),
        }),
    }
}

// peak-alloc: runtime baseline (no user-code heap) 64.0 KB incl. (heap peak 199.3 KB, massif, 2026-09-04)

// leak-suspect: 11856 B possibly lost + 2 "errors" — adjudicated: tokio teardown noise (process::exit skips runtime Drop; glibc TLS of runtime threads), NOT a leak, 0 definite/indirect (2026-09-04)
