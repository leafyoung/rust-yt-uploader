//! YouTube video lister CLI
//!
//! This binary lists all videos from the authenticated user's YouTube channel
//! with their details including video ID, title, description, status, recording date,
//! language, and audio language.
//!
//! The output can be formatted as JSON, JSONL, or table format for easy consumption
//! by other tools (e.g., for downloading or updating video metadata).

use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::YouTubeClient;
use std::path::PathBuf;
use tracing::info;

/// Output format for video listing
#[derive(Debug, Clone, Copy, PartialEq)]
enum OutputFormat {
    /// JSON format - all videos in a single array
    Json,
    /// JSONL format - one JSON object per line
    Jsonl,
    /// Table format - human-readable table
    Table,
}

impl std::str::FromStr for OutputFormat {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(OutputFormat::Json),
            "jsonl" => Ok(OutputFormat::Jsonl),
            "table" => Ok(OutputFormat::Table),
            _ => Err(anyhow::anyhow!(
                "Invalid output format: {}. Supported: json, jsonl, table",
                s
            )),
        }
    }
}

/// YouTube video lister CLI
#[derive(Parser)]
#[command(name = "yt-list")]
#[command(about = "List all videos from your YouTube channel with their details")]
#[command(long_about = r#"
List all videos from your YouTube channel with their details.

The tool retrieves comprehensive information about each video including:
- Video ID (needed for downloading)
- Title, description, category, status
- Upload date and recording date
- Default language and audio language
- Tags/keywords

Output formats:
- json: All videos in a single JSON array
- jsonl: One JSON object per line (useful for streaming/piping)
- table: Human-readable table format (default)

This information can be used for:
1. Downloading videos (using video IDs)
2. Updating video metadata (recording date, language, audio language)
"#)]
struct Cli {
    /// Output format: json, jsonl, or table
    #[arg(short, long, default_value = "table")]
    format: String,

    /// Save output to a file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Show only videos with specific privacy status (public, private, unlisted)
    #[arg(long)]
    status: Option<String>,

    /// Show only video IDs (one per line)
    #[arg(long)]
    ids_only: bool,
}

/// Initialize tracing/logging
fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .init();
}

/// Format videos as JSON
fn format_as_json(videos: &[rust_yt_uploader::VideoDetails]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&videos)?)
}

/// Format videos as JSONL (one JSON per line)
fn format_as_jsonl(videos: &[rust_yt_uploader::VideoDetails]) -> Result<String> {
    let lines: Result<Vec<String>> = videos
        .iter()
        .map(|v| serde_json::to_string(v).map_err(|e| anyhow::anyhow!(e)))
        .collect();
    Ok(lines?.join("\n"))
}

/// Format videos as table
fn format_as_table(videos: &[rust_yt_uploader::VideoDetails]) -> String {
    if videos.is_empty() {
        return "No videos found.".to_string();
    }

    let mut output = String::new();

    // Header
    output.push_str(
        "VIDEO ID             | TITLE                              | STATUS   | RECORDING DATE\n",
    );
    output.push_str("--------------------+------------------------------------+----------+--------------------\n");

    // Rows
    for video in videos {
        let title = if video.title.len() > 34 {
            format!("{}...", &video.title[..31])
        } else {
            video.title.clone()
        };

        let recording_date = video.recording_date.as_deref().unwrap_or("N/A");

        output.push_str(&format!(
            "{:<20} | {:<34} | {:<8} | {}\n",
            video.id, title, video.status, recording_date
        ));
    }

    output
}

/// Format videos as IDs only
fn format_as_ids_only(videos: &[rust_yt_uploader::VideoDetails]) -> String {
    videos
        .iter()
        .map(|v| v.id.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let cli = Cli::parse();

    info!("Starting YouTube video lister");

    // Parse output format
    let format: OutputFormat = cli.format.parse()?;

    // Initialize YouTube client
    let uploader = YouTubeClient::new().await?;

    info!("Fetching videos from your channel");

    // Fetch all videos
    let mut videos = uploader.list_all_videos().await?;

    // Filter by status if requested
    if let Some(status_filter) = cli.status {
        info!("Filtering videos by status: {}", status_filter);
        videos.retain(|v| v.status.to_lowercase() == status_filter.to_lowercase());
    }

    info!("Retrieved {} video(s)", videos.len());

    // Format output
    let output_text = if cli.ids_only {
        format_as_ids_only(&videos)
    } else {
        match format {
            OutputFormat::Json => format_as_json(&videos)?,
            OutputFormat::Jsonl => format_as_jsonl(&videos)?,
            OutputFormat::Table => format_as_table(&videos),
        }
    };

    // Write output
    if let Some(output_path) = cli.output {
        std::fs::write(&output_path, &output_text)?;
        info!("Output written to: {}", output_path.display());
    } else {
        println!("{}", output_text);
    }

    info!("Done");
    Ok(())
}
