#!/usr/bin/env rust-script
//! YouTube video uploader with OAuth 2.0 authentication.
//!
//! This is a Rust implementation of the Python YouTube uploader, providing
//! same functionality with improved performance and memory safety.

use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{BatchConfigRoot, ConfigFormat, LegacyConfigRoot};
use rust_yt_uploader::{
    upload_batch_concurrent, upload_batch_sequential, upload_legacy_sequential,
};
use std::path::PathBuf;
use tracing::info;

/// YouTube video uploader CLI
#[derive(Parser)]
#[command(name = "rust-yt-upload")]
#[command(about = "Upload videos to YouTube from a YAML configuration file")]
#[command(long_about = r#"
Upload videos to YouTube from a YAML configuration file.

Supports two YAML schema formats:
- Legacy: 'videos' array with per-video configuration
- Batch: 'common' config + separate 'titles' and 'files' arrays

Async mode uploads multiple videos concurrently for better performance.
"#)]
struct Cli {
    /// YAML configuration file
    #[arg(short, long, value_name = "FILE")]
    file: PathBuf,

    /// Use async upload for concurrent processing
    #[arg(long)]
    r#async: bool,

    /// Maximum number of concurrent uploads (only used with --async)
    #[arg(long, default_value = "3")]
    concurrent: usize,

    /// Show progress bars during upload
    #[arg(long)]
    progress: bool,
}

/// Detect which YAML schema format is being used.
///
/// # Arguments
/// * `config` - Raw YAML configuration as a string
///
/// # Returns
/// * `ConfigFormat` - Either Legacy or Batch
///
/// # Errors
/// * Returns error if schema cannot be determined
fn detect_yaml_schema(config: &str) -> Result<ConfigFormat> {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(config)?;

    if let Some(mapping) = value.as_mapping() {
        if mapping.contains_key("videos") {
            return Ok(ConfigFormat::Legacy);
        }

        if mapping.contains_key("common")
            && mapping.contains_key("titles")
            && mapping.contains_key("files")
        {
            return Ok(ConfigFormat::Batch);
        }
    }

    anyhow::bail!(
        "Unable to determine YAML schema. Expected either 'videos' key (legacy) or 'common', 'titles', and 'files' keys (batch)."
    );
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

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let cli = Cli::parse();

    info!("Starting YouTube uploader");

    // Read and parse configuration file
    let config_content = std::fs::read_to_string(&cli.file).map_err(|e| {
        anyhow::anyhow!("Failed to read config file '{}': {}", cli.file.display(), e)
    })?;

    let schema_type = detect_yaml_schema(&config_content)?;

    match schema_type {
        ConfigFormat::Legacy => {
            info!("Detected legacy YAML schema format");
            let config: LegacyConfigRoot = serde_yaml_ng::from_str(&config_content)
                .map_err(|e| anyhow::anyhow!("Failed to parse legacy config: {}", e))?;

            upload_legacy_sequential(config, cli.progress).await?;
        }
        ConfigFormat::Batch => {
            info!("Detected batch YAML schema format");
            let config: BatchConfigRoot = serde_yaml_ng::from_str(&config_content)
                .map_err(|e| anyhow::anyhow!("Failed to parse batch config: {}", e))?;

            if cli.r#async {
                info!(
                    "Using async upload mode with {} concurrent uploads",
                    cli.concurrent
                );
                let video_ids =
                    upload_batch_concurrent(config, cli.concurrent, cli.progress).await?;
                info!("All {} videos uploaded successfully", video_ids.len());
            } else {
                info!("Using sequential upload mode");
                upload_batch_sequential(config, cli.progress).await?;
            }
        }
    }

    info!("All videos processed successfully");
    Ok(())
}
