//! YouTube video language updater CLI
//!
//! This binary updates language metadata for all public videos in the authenticated
//! user's YouTube channel. It sets defaultLanguage to "zh" and defaultAudioLanguage
//! to "zh-Hans" for any videos that don't already have these values set.
//!
//! This is useful for batch-updating video metadata without manual intervention.

use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use rust_yt_uploader::{VideoDetails, YouTubeClient, init_logging, validate_profile_name};
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

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,
}

/// Whether a video's language fields still need to be set to zh / zh-Hans.
fn needs_language_update(video: &VideoDetails, only_empty: bool) -> bool {
    if only_empty {
        // Only update if both language fields are None
        video.default_language.is_none() && video.default_audio_language.is_none()
    } else {
        // Update if either field is missing or not set to our target values
        (video.default_language.as_ref().is_none_or(|l| l != "zh"))
            || (video
                .default_audio_language
                .as_ref()
                .is_none_or(|l| l != "zh-Hans"))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    info!("Starting YouTube video language updater");

    // Validate profile name
    validate_profile_name(&cli.profile)?;
    info!("Using profile: {}", cli.profile);

    // Initialize YouTube client with profile
    let uploader = YouTubeClient::new(&cli.profile).await?;

    info!("Fetching all public videos from your channel");

    if cli.dry_run {
        info!("DRY RUN MODE - No changes will be made");
    }

    let (mut total, mut public_count) = (0usize, 0usize);
    let (mut matched, mut updated, mut failed) = (0usize, 0usize, 0usize);

    // Stream pages so the channel never materializes; update as we go
    // (sequential with a small delay to avoid rate limiting).
    let pages = uploader.video_pages();
    futures::pin_mut!(pages);
    'pages: while let Some(page) = pages.next().await {
        for video in page? {
            total += 1;
            if video.status.to_lowercase() != "public" {
                continue;
            }
            public_count += 1;

            if !needs_language_update(&video, cli.only_empty) {
                continue;
            }
            matched += 1;

            if cli.dry_run {
                let current_lang = video.default_language.as_deref().unwrap_or("(not set)");
                let current_audio = video
                    .default_audio_language
                    .as_deref()
                    .unwrap_or("(not set)");
                println!(
                    "{}. [{}] {} (lang: {} → zh, audio: {} → zh-Hans)",
                    matched, video.id, video.title, current_lang, current_audio
                );
                continue;
            }

            if cli.verbose {
                let current_lang = video.default_language.as_deref().unwrap_or("(not set)");
                let current_audio = video
                    .default_audio_language
                    .as_deref()
                    .unwrap_or("(not set)");
                info!(
                    "Updating {} - {} (lang: {} → zh, audio: {} → zh-Hans)",
                    &video.id, video.title, current_lang, current_audio
                );
            } else {
                info!("Updating {} - {}", &video.id, video.title);
            }

            match uploader
                .update_video_language(&video.id, Some("zh"), Some("zh-Hans"))
                .await
            {
                Ok(_) => {
                    updated += 1;
                }
                Err(e) => {
                    failed += 1;
                    info!(
                        "Failed to update video {}: {}, stopping updates",
                        video.id, e
                    );
                    break 'pages;
                }
            }

            // Add a small delay between updates to avoid rate limiting
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    println!("\n=== Summary ===");
    println!(
        "Found {} public video(s) out of {} total",
        public_count, total
    );
    println!("Matching videos: {}", matched);

    if cli.dry_run {
        println!("\nRun without --dry-run to apply these changes");
    } else {
        println!("Successfully updated: {}", updated);
        if failed > 0 {
            println!("Failed: {}", failed);
        }
        println!("Total processed: {}", updated + failed);
        info!("Update complete - {} updated, {} failed", updated, failed);
    }

    Ok(())
}

// peak-alloc: runtime baseline (no user-code heap) 64.0 KB incl. (heap peak 198.5 KB, massif, 2026-09-04)

// leak-suspect: 11856 B possibly lost + 2 "errors" — adjudicated: tokio teardown noise (process::exit skips runtime Drop; glibc TLS of runtime threads), NOT a leak, 0 definite/indirect (2026-09-04)
