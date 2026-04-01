use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::fs;
use std::path::Path;
use tracing::info;

/// YouTube video comment poster CLI
#[derive(Parser)]
#[command(name = "yt-add-pin-comment")]
#[command(about = "Post a comment to a YouTube video from a text file")]
#[command(long_about = r#"
Post a comment to a YouTube video from a text file.

This tool reads content from a .txt file and posts it as a comment to a specified
YouTube video. The comment can optionally be pinned (featured) on the video.

Usage examples:
  yt-add-pin-comment <video_id> <comment_file.txt>
  yt-add-pin-comment abc123 my_comment.txt
  yt-add-pin-comment abc123 my_comment.txt --pin
  yt-add-pin-comment abc123 my_comment.txt --pin --skip-if-pinned
"#)]
struct Cli {
    /// YouTube video ID to post comment to
    video_id: String,

    /// Path to text file containing the comment
    comment_file: String,

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
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();

    // Validate profile name
    validate_profile_name(&cli.profile)?;
    info!("Using profile: {}", cli.profile);

    let comment_path = Path::new(&cli.comment_file);
    if !comment_path.exists() {
        anyhow::bail!("Comment file not found: {}", cli.comment_file);
    }

    let comment_text = fs::read_to_string(&cli.comment_file)?;

    let comment_text: String = comment_text
        .lines()
        .filter(|line| !line.contains("-- end of file --"))
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    let comment_text = comment_text.trim();

    if comment_text.is_empty() {
        anyhow::bail!("Comment file is empty: {}", cli.comment_file);
    }

    println!("Reading comment from: {}", cli.comment_file);
    println!("Target video: {}", cli.video_id);
    if cli.pin {
        println!("Comment will be pinned (featured)");
    }
    println!();
    println!("Comment to post:");
    println!("─────────────────────────────────────────");
    println!("{}", comment_text);
    println!("─────────────────────────────────────────");
    println!();

    let client = YouTubeClient::new(&cli.profile).await?;

    // Bail out early if the video already has a pinned comment and the flag is set
    if cli.skip_if_pinned {
        println!("Checking for existing pinned comment on video...");
        match client.has_pinned_comment(&cli.video_id).await {
            Ok(true) => {
                println!();
                println!("⚠ Pinned comment already exists on video {}.", cli.video_id);
                if cli.force {
                    println!("Proceeding because --force was specified.");
                } else {
                    println!("Skipping post because --skip-if-pinned was specified.");
                    return Ok(());
                }
            }
            Ok(false) => {
                println!("No pinned comment found. Proceeding...");
            }
            Err(e) => {
                // Detection failed
                println!("⚠ Warning: Could not reliably detect pinned comment: {}", e);
                if cli.force {
                    println!("Proceeding because --force was specified.");
                } else {
                    println!("Skipping post to be safe. Use --force to override.");
                    return Ok(());
                }
            }
        }
        println!();
    }

    // Check if comment already exists on the video
    println!("Checking for duplicate comment on video...");
    let already_exists = client.comment_exists(&cli.video_id, comment_text).await?;

    if already_exists {
        println!();
        println!("⚠ WARNING: A comment with identical text already exists on this video!");
        println!("Skipping post to prevent duplicate comment.");
        return Ok(());
    }

    println!("No duplicate found. Proceeding with post...");
    println!();

    let comment_id = client.post_comment(&cli.video_id, comment_text).await?;

    println!("✓ Successfully posted comment to video {}", cli.video_id);
    println!("  Comment ID: {}", comment_id);

    if cli.pin {
        match client.pin_comment(&comment_id).await {
            Ok(()) => {
                println!("✓ Successfully pinned comment");
            }
            Err(e) => {
                println!();
                println!("⚠ Failed to pin comment after retries: {}", e);
                println!("Comment was posted successfully; exiting gracefully...");
            }
        }
    }

    Ok(())
}
