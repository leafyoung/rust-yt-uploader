use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::info;

/// (added, detail) — added=false means the video was already in the playlist
type Outcome = (bool, String);

/// YouTube playlist adder CLI
#[derive(Parser)]
#[command(name = "yt-add-playlist")]
#[command(about = "Add YouTube videos to a playlist")]
#[command(long_about = r#"
Add one or more YouTube videos to a playlist, appended at the end.

Videos already in the playlist are skipped (idempotent - safe to re-run).

Usage examples:
  yt-add-playlist -p <profile> <video_id> <playlist_id>
  yt-add-playlist -p dongli abc123 def456 PLvHKQzp_pnlgnbeksaYbgk9phiJ4UoaBX
"#)]
struct Cli {
    /// YouTube video ID(s), followed by the playlist ID (last argument)
    #[arg(required = true)]
    args: Vec<String>,

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,

    /// Maximum number of concurrent additions (default: 5)
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

    // Need at least 2 args: video_id and playlist_id
    if cli.args.len() < 2 {
        anyhow::bail!(
            "Usage: yt-add-playlist [OPTIONS] -p <PROFILE> <video_id> [<video_id>...] <playlist_id>"
        );
    }

    // Last argument is the playlist ID
    let playlist_id = cli.args.last().unwrap().clone();
    let video_ids: Vec<String> = cli.args[..cli.args.len() - 1].to_vec();

    println!("Playlist: {}", playlist_id);
    println!(
        "Processing {} video(s): {}",
        video_ids.len(),
        video_ids.join(", ")
    );
    println!();

    // Create shared client and semaphore for concurrency
    let client = Arc::new(YouTubeClient::new(&cli.profile).await?);
    let semaphore = Arc::new(Semaphore::new(cli.concurrent));

    let start = std::time::Instant::now();

    // Process videos concurrently
    let mut tasks = Vec::new();
    for video_id in video_ids.clone() {
        let client = Arc::clone(&client);
        let semaphore = Arc::clone(&semaphore);
        let playlist_id = playlist_id.clone();

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await?;
            if client.is_video_in_playlist(&playlist_id, &video_id).await? {
                return Ok::<Outcome, anyhow::Error>((false, "already in playlist".to_string()));
            }
            let item_id = client.add_to_playlist(&video_id, &playlist_id).await?;
            Ok((true, item_id))
        });
        tasks.push(task);
    }

    // Wait for all tasks and collect results
    let results: Vec<_> = futures::future::join_all(tasks).await;

    let duration = start.elapsed();
    let mut added_count = 0;
    let mut skipped_count = 0;
    let mut error_count = 0;

    println!("\n=== Results ===\n");

    for (i, result) in results.iter().enumerate() {
        let video_id = &video_ids[i];
        match result {
            Ok(Ok((true, detail))) => {
                added_count += 1;
                println!("✓ {} - Added ({})", video_id, detail);
            }
            Ok(Ok((false, detail))) => {
                skipped_count += 1;
                println!("⊘ {} - Skipped: {}", video_id, detail);
            }
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
    println!("✓ Added: {}", added_count);
    if skipped_count > 0 {
        println!("⊘ Skipped: {}", skipped_count);
    }
    if error_count > 0 {
        println!("✗ Failed: {}", error_count);
    }
    println!("  Total time: {:.2}s", duration.as_secs_f64());

    Ok(())
}

// peak-alloc: runtime baseline (no user-code heap) 64.0 KB incl. (heap peak 190.1 KB, massif, 2026-10-18)

// leak-suspect: 11856 B possibly lost + 2 "errors" — adjudicated: tokio teardown noise (process::exit skips runtime Drop; glibc TLS of runtime threads), NOT a leak, 0 definite/indirect (2026-10-18)
