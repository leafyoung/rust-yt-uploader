//! YouTube video language updater CLI
//!
//! This binary updates language metadata for all public videos in the authenticated
//! user's YouTube channel. It sets defaultLanguage to "zh" and defaultAudioLanguage
//! to "zh-Hans" for any videos that don't already have these values set.
//!
//! This is useful for batch-updating video metadata without manual intervention.

use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::info;

/// YouTube video language updater CLI
#[derive(Parser)]
#[command(name = "yt-update-lang")]
#[command(about = "Update language metadata for all public videos")]
#[command(long_about = r#"
Update language metadata for all public videos in your YouTube channel.

This tool automatically sets:
- defaultLanguage: zh (Chinese)
- defaultAudioLanguage: zh-Hans (Simplified Chinese)

For any public videos that don't already have these values set, it will
skip videos that already have their language metadata configured.

This is useful for:
1. Batch-updating newly uploaded videos
2. Ensuring consistent language metadata across your channel
3. Setting up correct language information for accessibility
"#)]
struct Cli {
    /// Dry run mode - show what would be updated without making changes
    #[arg(long)]
    dry_run: bool,

    /// Show verbose output including each video processed
    #[arg(short, long)]
    verbose: bool,

    /// Only update videos that have no language set (not even empty)
    #[arg(long)]
    only_empty: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    info!("Starting YouTube video language updater");

    // Initialize YouTube client
    let uploader = YouTubeClient::new().await?;

    info!("Fetching all public videos from your channel");

    // Fetch all videos
    let videos = uploader.list_all_videos().await?;

    // Filter for public videos
    let public_videos: Vec<_> = videos
        .iter()
        .filter(|v| v.status.to_lowercase() == "public")
        .collect();

    info!(
        "Found {} public video(s) out of {} total",
        public_videos.len(),
        videos.len()
    );

    if public_videos.is_empty() {
        info!("No public videos to update");
        return Ok(());
    }

    // Count videos that need updating
    let needs_update: Vec<_> = public_videos
        .iter()
        .filter(|v| {
            // Check if we should update this video
            if cli.only_empty {
                // Only update if both language fields are None
                v.default_language.is_none() && v.default_audio_language.is_none()
            } else {
                // Update if either field is missing or not set to our target values
                (v.default_language.as_ref().is_none_or(|l| l != "zh"))
                    || (v
                        .default_audio_language
                        .as_ref()
                        .is_none_or(|l| l != "zh-Hans"))
            }
        })
        .collect();

    info!(
        "Found {} video(s) that need language metadata update",
        needs_update.len()
    );

    if needs_update.is_empty() {
        info!("All public videos already have the correct language metadata");
        return Ok(());
    }

    // Show summary in dry run mode
    if cli.dry_run {
        info!("DRY RUN MODE - No changes will be made");
        println!("\nWould update {} video(s):\n", needs_update.len());
        for (idx, video) in needs_update.iter().enumerate() {
            let current_lang = video.default_language.as_deref().unwrap_or("(not set)");
            let current_audio = video
                .default_audio_language
                .as_deref()
                .unwrap_or("(not set)");
            println!(
                "{}. [{}] {} (lang: {} → zh, audio: {} → zh-Hans)",
                idx + 1,
                &video.id,
                video.title,
                current_lang,
                current_audio
            );
        }
        println!("\nRun without --dry-run to apply these changes");
        return Ok(());
    }

    // Update videos
    info!("Starting to update {} video(s)", needs_update.len());
    let updated_count = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));

    for (idx, video) in needs_update.iter().enumerate() {
        let progress = format!("[{}/{}]", idx + 1, needs_update.len());

        if cli.verbose {
            let current_lang = video.default_language.as_deref().unwrap_or("(not set)");
            let current_audio = video
                .default_audio_language
                .as_deref()
                .unwrap_or("(not set)");
            info!(
                "{} Updating {} - {} (lang: {} → zh, audio: {} → zh-Hans)",
                progress, &video.id, video.title, current_lang, current_audio
            );
        } else {
            info!("{} Updating {} - {}", progress, &video.id, video.title);
        }

        match uploader
            .update_video_language(&video.id, Some("zh"), Some("zh-Hans"))
            .await
        {
            Ok(_) => {
                updated_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                failed_count.fetch_add(1, Ordering::Relaxed);
                info!(
                    "Failed to update video {}: {}, stopping updates",
                    video.id, e
                );
                break;
            }
        }

        // Add a small delay between updates to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    let updated = updated_count.load(Ordering::Relaxed);
    let failed = failed_count.load(Ordering::Relaxed);

    println!("\n=== Update Summary ===");
    println!("Successfully updated: {}", updated);
    if failed > 0 {
        println!("Failed: {}", failed);
    }
    println!("Total processed: {}", updated + failed);

    info!("Update complete - {} updated, {} failed", updated, failed);

    Ok(())
}
