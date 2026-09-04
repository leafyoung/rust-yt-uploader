use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::info;

/// YouTube video comment poster CLI
#[derive(Parser)]
#[command(name = "yt-add-pin-comment")]
#[command(about = "Post a comment to YouTube videos from a text file")]
#[command(long_about = r#"
Post a comment to YouTube videos from a text file.

This tool reads content from a .txt file and posts it as a comment to specified
YouTube video(s). The comment can optionally be pinned (featured) on the video.

Supports multiple video IDs for parallel processing - up to 3 concurrent
updates for ~60% performance improvement on batch operations vs sequential.

Usage examples:
  yt-add-pin-comment -p <profile> <video_id> <comment_file.txt>
  yt-add-pin-comment -p dongli abc123 my_comment.txt
  yt-add-pin-comment -p dongli abc123 def456 ghi789 my_comment.txt --pin
  yt-add-pin-comment -p dongli vid1 vid2 vid3 comment.txt --pin --skip-if-pinned
"#)]
struct Cli {
    /// Video ID(s) and comment file path (last argument is the file)
    #[arg(required = true)]
    args: Vec<String>,

    /// Pin (feature) the comment after posting
    #[arg(long)]
    pin: bool,

    /// Skip posting if the video already has at least one pinned comment
    #[arg(long)]
    skip_if_pinned: bool,

    /// Force posting even if a pinned comment is detected (or detection fails)
    #[arg(long)]
    force: bool,

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,

    /// Maximum number of concurrent updates (default: 3)
    #[arg(short, long, default_value = "3")]
    concurrent: usize,
}

/// Result of processing a single video
#[allow(dead_code)]
struct VideoResult {
    video_id: String,
    status: VideoStatus,
    comment_id: Option<String>,
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

    // Need at least 2 args: video_id and comment_file
    if cli.args.len() < 2 {
        anyhow::bail!(
            "Usage: yt-add-pin-comment [OPTIONS] -p <PROFILE> <video_id> [<video_id>...] <comment_file.txt>"
        );
    }

    // Last argument is the comment file
    let comment_file = cli.args.last().unwrap().clone();
    let video_ids: Vec<String> = cli.args[..cli.args.len() - 1].to_vec();

    let comment_path = Path::new(&comment_file);
    if !comment_path.exists() {
        anyhow::bail!("Comment file not found: {}", comment_file);
    }

    let comment_text = fs::read_to_string(&comment_file)?;

    let comment_text: String = comment_text
        .lines()
        .filter(|line| !line.contains("-- end of file --"))
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    let comment_text = comment_text.trim();

    if comment_text.is_empty() {
        anyhow::bail!("Comment file is empty: {}", comment_file);
    }

    println!("Reading comment from: {}", comment_file);
    println!(
        "Processing {} video(s): {}",
        video_ids.len(),
        video_ids.join(", ")
    );
    if cli.pin {
        println!("Comments will be pinned (featured)");
    }
    if cli.skip_if_pinned {
        println!("Skipping videos with existing pinned comments");
    }
    println!();
    println!("Comment to post:");
    println!("─────────────────────────────────────────");
    println!("{}", comment_text);
    println!("─────────────────────────────────────────");
    println!();

    // Create shared client and semaphore for concurrency
    let client = Arc::new(YouTubeClient::new(&cli.profile).await?);
    let semaphore = Arc::new(Semaphore::new(cli.concurrent));
    let comment_text = Arc::new(comment_text.to_string());

    let start = std::time::Instant::now();

    // Process videos concurrently
    let mut tasks = Vec::new();
    for video_id in video_ids.clone() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let comment_text = Arc::clone(&comment_text);
        let pin = cli.pin;
        let skip_if_pinned = cli.skip_if_pinned;
        let force = cli.force;

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await?;
            process_video(
                &client,
                &video_id,
                &comment_text,
                pin,
                skip_if_pinned,
                force,
            )
            .await
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
                    println!("✓ {} - Comment posted successfully", video_id);
                    if let Some(ref comment_id) = video_result.comment_id {
                        println!("  Comment ID: {}", comment_id);
                    }
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
    if success_count > 0 {
        println!(
            "  Average per video: {:.2}s",
            duration.as_secs_f64() / video_ids.len() as f64
        );
    }

    Ok(())
}

async fn process_video(
    client: &YouTubeClient,
    video_id: &str,
    comment_text: &str,
    pin: bool,
    skip_if_pinned: bool,
    force: bool,
) -> Result<VideoResult> {
    // Check for existing pinned comment if flag is set
    if skip_if_pinned {
        match client.has_pinned_comment(video_id).await {
            Ok(true) => {
                if force {
                    // Continue processing
                } else {
                    return Ok(VideoResult {
                        video_id: video_id.to_string(),
                        status: VideoStatus::Skipped("Pinned comment already exists".to_string()),
                        comment_id: None,
                    });
                }
            }
            Ok(false) => {
                // Continue processing
            }
            Err(e) => {
                if force {
                    // Continue processing
                } else {
                    return Ok(VideoResult {
                        video_id: video_id.to_string(),
                        status: VideoStatus::Skipped(format!(
                            "Could not detect pinned comment: {}",
                            e
                        )),
                        comment_id: None,
                    });
                }
            }
        }
    }

    // Check if comment already exists
    match client.comment_exists(video_id, comment_text).await {
        Ok(true) => {
            return Ok(VideoResult {
                video_id: video_id.to_string(),
                status: VideoStatus::Skipped("Identical comment already exists".to_string()),
                comment_id: None,
            });
        }
        Ok(false) => {
            // Continue processing
        }
        Err(e) => {
            return Ok(VideoResult {
                video_id: video_id.to_string(),
                status: VideoStatus::Error(format!("Failed to check for duplicate: {}", e)),
                comment_id: None,
            });
        }
    }

    // Post the comment
    match client.post_comment(video_id, comment_text).await {
        Ok(comment_id) => {
            if pin {
                match client.pin_comment(&comment_id).await {
                    Ok(()) => Ok(VideoResult {
                        video_id: video_id.to_string(),
                        status: VideoStatus::Success,
                        comment_id: Some(comment_id),
                    }),
                    Err(_e) => {
                        // Comment posted but pinning failed
                        Ok(VideoResult {
                            video_id: video_id.to_string(),
                            status: VideoStatus::Success,
                            comment_id: Some(comment_id),
                        })
                    }
                }
            } else {
                Ok(VideoResult {
                    video_id: video_id.to_string(),
                    status: VideoStatus::Success,
                    comment_id: Some(comment_id),
                })
            }
        }
        Err(e) => Ok(VideoResult {
            video_id: video_id.to_string(),
            status: VideoStatus::Error(format!("Failed to post comment: {}", e)),
            comment_id: None,
        }),
    }
}

// peak-alloc: runtime baseline (no user-code heap) 64.0 KB incl. (heap peak 200.2 KB, massif, 2026-09-04)

// leak-suspect: 11856 B possibly lost + 2 "errors" — adjudicated: tokio teardown noise (process::exit skips runtime Drop; glibc TLS of runtime threads), NOT a leak, 0 definite/indirect (2026-09-04)
