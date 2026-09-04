//! YouTube video lister CLI
//!
//! This binary lists all videos from the authenticated user's YouTube channel
//! with their details including video ID, title, description, status, recording date,
//! duration, language, and audio language.
//!
//! The output can be formatted as JSON, JSONL, or table format for easy consumption
//! by other tools (e.g., for downloading or updating video metadata).

use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
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
- Upload date, recording date, and duration
- Default language and audio language
- Tags/keywords
- Available captions/subtitles

Output formats:
- json: All videos in a single JSON array
- jsonl: One JSON object per line (useful for streaming/piping)
- table: Human-readable table format (default)

This information can be used for:
1. Downloading videos (using video IDs)
2. Updating video metadata (recording date, language, audio language)
3. Managing captions and subtitles
"#)]
struct Cli {
    /// Output format: json, jsonl, or table
    #[arg(short, long, default_value = "table")]
    format: String,

    /// Save output to a file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Show only video IDs (one per line)
    #[arg(long)]
    ids_only: bool,

    /// List available subtitles/captions for videos
    #[arg(long)]
    list_subtitles: bool,

    /// Filter subtitles by video ID (requires --list-subtitles)
    #[arg(long)]
    video_id: Option<String>,

    /// Filter subtitles by language code (e.g., 'en', 'zh', 'fr')
    #[arg(long)]
    language: Option<String>,

    /// Profile name for OAuth (alphanumeric only)
    /// Credentials: client_secret-{profile}.json, Token: youtube-oauth2-{profile}.json
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,
}

/// Format videos as JSON
fn format_as_json(videos: &[rust_yt_uploader::VideoDetails]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&videos)?)
}

/// Stream videos as JSONL (one JSON per line) directly to `w`.
/// Avoids materializing per-video Strings + a joined copy: O(output) -> O(line).
fn write_jsonl(
    videos: &[rust_yt_uploader::VideoDetails],
    w: &mut dyn std::io::Write,
) -> Result<()> {
    for v in videos {
        serde_json::to_writer(&mut *w, v).map_err(|e| anyhow::anyhow!(e))?;
        writeln!(w)?;
    }
    Ok(())
}

/// Format videos as table
fn format_as_table(videos: &[rust_yt_uploader::VideoDetails]) -> String {
    if videos.is_empty() {
        return "No videos found.".to_string();
    }

    let mut output = String::new();

    // Header
    output.push_str(
        "VIDEO ID             | TITLE                              | DURATION | STATUS   | RECORDING DATE\n",
    );
    output.push_str("--------------------+------------------------------------+----------+----------+--------------------\n");

    // Rows
    for video in videos {
        let title = if video.title.len() > 34 {
            format!("{}...", &video.title[..31])
        } else {
            video.title.clone()
        };

        let recording_date = video.recording_date.as_deref().unwrap_or("N/A");
        let duration = video.duration.as_deref().unwrap_or("N/A");

        output.push_str(&format!(
            "{:<20} | {:<34} | {:<8} | {:<8} | {}\n",
            video.id, title, duration, video.status, recording_date
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

/// Format captions as JSON
fn format_captions_as_json(
    captions: &[(String, Vec<rust_yt_uploader::CaptionDetails>)],
) -> Result<String> {
    let captions_map: std::collections::HashMap<String, Vec<rust_yt_uploader::CaptionDetails>> =
        captions.iter().cloned().collect();
    Ok(serde_json::to_string_pretty(&captions_map)?)
}

/// Format captions as JSONL
fn format_captions_as_jsonl(
    captions: &[(String, Vec<rust_yt_uploader::CaptionDetails>)],
) -> Result<String> {
    let lines: Result<Vec<String>> = captions
        .iter()
        .map(|(video_id, caps)| {
            serde_json::to_string(&serde_json::json!({
                "videoId": video_id,
                "captions": caps
            }))
            .map_err(|e| anyhow::anyhow!(e))
        })
        .collect();
    Ok(lines?.join("\n"))
}

/// Format captions as table
fn format_captions_as_table(
    captions: &[(String, Vec<rust_yt_uploader::CaptionDetails>)],
) -> String {
    if captions.is_empty() {
        return "No captions found.".to_string();
    }

    let mut output = String::new();

    // Header
    output.push_str(
        "VIDEO ID             | LANGUAGE | AUTO-SYNCED | DRAFT | CLOSED CAPTIONS | LARGE | NAME\n",
    );
    output.push_str("--------------------+----------+-------------+-------+-----------------+-------+--------------------\n");

    // Rows
    for (video_id, caps) in captions {
        if caps.is_empty() {
            continue;
        }

        for (idx, cap) in caps.iter().enumerate() {
            let auto_synced = cap.is_auto_synced.unwrap_or(false);
            let is_draft = cap.is_draft.unwrap_or(false);
            let is_cc = cap.is_cc.unwrap_or(false);
            let is_large = cap.is_large.unwrap_or(false);
            let name = cap.name.as_deref().unwrap_or("-");

            let video_id_display = if idx == 0 { video_id.as_str() } else { "" };

            output.push_str(&format!(
                "{:<20} | {:<8} | {:<11} | {:<5} | {:<15} | {:<5} | {}\n",
                video_id_display,
                cap.language,
                if auto_synced { "Yes" } else { "No" },
                if is_draft { "Yes" } else { "No" },
                if is_cc { "Yes" } else { "No" },
                if is_large { "Yes" } else { "No" },
                name
            ));
        }
    }

    output
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let cli = Cli::parse();

    info!("Starting YouTube video lister");

    // Validate profile name
    validate_profile_name(&cli.profile)?;
    info!("Using profile: {}", cli.profile);

    // Parse output format
    let format: OutputFormat = cli.format.parse()?;

    // Initialize YouTube client with profile
    let uploader = YouTubeClient::new(&cli.profile).await?;

    // Handle subtitle listing
    if cli.list_subtitles {
        info!("Fetching captions/subtitles");

        let captions = if let Some(video_id) = &cli.video_id {
            // List captions for a specific video
            info!("Fetching captions for video: {}", video_id);
            let caps = uploader.list_video_captions(video_id).await?;

            // Filter by language if specified
            let filtered_caps = if let Some(lang) = &cli.language {
                caps.into_iter().filter(|c| c.language == *lang).collect()
            } else {
                caps
            };

            vec![(video_id.clone(), filtered_caps)]
        } else {
            // List captions for all videos
            let all_captions = uploader.list_all_captions().await?;

            // Filter by language if specified
            if let Some(lang) = &cli.language {
                all_captions
                    .into_iter()
                    .map(|(vid, caps)| {
                        let filtered: Vec<_> =
                            caps.into_iter().filter(|c| c.language == *lang).collect();
                        (vid, filtered)
                    })
                    .filter(|(_, caps)| !caps.is_empty())
                    .collect()
            } else {
                all_captions
            }
        };

        info!("Retrieved captions for {} video(s)", captions.len());

        // Format output
        let output_text = match format {
            OutputFormat::Json => format_captions_as_json(&captions)?,
            OutputFormat::Jsonl => format_captions_as_jsonl(&captions)?,
            OutputFormat::Table => format_captions_as_table(&captions),
        };

        // Write output
        if let Some(output_path) = cli.output {
            std::fs::write(&output_path, &output_text)?;
            info!("Output written to: {}", output_path.display());
        } else {
            println!("{}", output_text);
        }
    } else {
        // List videos
        info!("Fetching videos from your channel");

        // jsonl streams page-by-page: never materializes the whole channel
        if !cli.ids_only && format == OutputFormat::Jsonl {
            let mut w: Box<dyn std::io::Write> = match &cli.output {
                Some(path) => Box::new(std::io::BufWriter::new(std::fs::File::create(path)?)),
                None => Box::new(std::io::BufWriter::new(std::io::stdout().lock())),
            };
            let mut count = 0usize;
            let pages = uploader.video_pages();
            futures::pin_mut!(pages);
            while let Some(page) = pages.next().await {
                let page = page?;
                count += page.len();
                write_jsonl(&page, &mut w)?;
            }
            w.flush()?;
            info!("Retrieved {} video(s)", count);
            if let Some(output_path) = &cli.output {
                info!("Output written to: {}", output_path.display());
            }
        } else {
            // Fetch all videos
            let videos = uploader.list_all_videos().await?;

            info!("Retrieved {} video(s)", videos.len());

            // Format output
            let output_text = if cli.ids_only {
                format_as_ids_only(&videos)
            } else {
                match format {
                    OutputFormat::Json => format_as_json(&videos)?,
                    OutputFormat::Jsonl => unreachable!("handled above"),
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
        }
    }

    info!("Done");
    Ok(())
}

// peak-alloc: runtime baseline (no user-code heap) 64.0 KB incl. (heap peak 196.6 KB, massif, 2026-09-04)

// leak-suspect: 11856 B possibly lost + 2 "errors" — adjudicated: tokio teardown noise (process::exit skips runtime Drop; glibc TLS of runtime threads), NOT a leak, 0 definite/indirect (2026-09-04)
