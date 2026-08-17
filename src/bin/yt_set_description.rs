use anyhow::Result;
use clap::Parser;
use rust_yt_uploader::{YouTubeClient, init_logging, validate_profile_name};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// YouTube video description SET (full overwrite) CLI.
///
/// Replaces a video's description with the exact content of a text file.
/// Unlike yt-append-description, this does NOT append or dedup — it overwrites.
/// Use to repair duplicated / corrupted descriptions by rebuilding from
/// canonical source files.
///
/// With --verify, after the PUT the tool re-GETs the description and asserts it
/// matches what was sent (modulo trailing whitespace). Retries on read-after-
/// write latency. A verify failure is a hard error for that video (non-zero),
/// so callers that gate a marker on the exit code will retry instead of
/// silently trusting the write.
#[derive(Parser)]
#[command(name = "yt-set-description")]
#[command(about = "Overwrite video descriptions from a text file (full replace)")]
struct Cli {
    /// Video ID(s) and content file path (last argument is the file).
    /// The same content is applied to every listed video.
    #[arg(required = true)]
    args: Vec<String>,

    /// Profile name for OAuth (alphanumeric only).
    #[arg(short, long, value_name = "PROFILE")]
    profile: String,

    /// Re-GET the description after writing and assert it matches what was sent.
    /// Retries on mismatch (YouTube read-after-write latency).
    #[arg(long)]
    verify: bool,

    /// Number of verify attempts before declaring failure (default 3).
    #[arg(long, default_value = "3")]
    verify_retries: usize,

    /// GET the live description first; skip the PUT if it already matches the
    /// file (normalized). If it differs, proceed to SET. Used by pipelines that
    /// re-enforce a canonical description every run without burning a write
    /// (or risking a rate-limit) when nothing changed.
    #[arg(long)]
    only_if_different: bool,
}

/// Normalize for comparison: CRLF -> LF, then trim leading/trailing whitespace.
/// YouTube occasionally trims trailing newlines, so we compare on trimmed form.
fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}

async fn verify_matches(
    client: &YouTubeClient,
    video_id: &str,
    expected: &str,
    retries: usize,
) -> Result<()> {
    let expected_n = normalize(expected);
    let mut last_diff: Option<(String, String)> = None;
    for attempt in 1..=retries {
        match client.get_video_description(video_id).await {
            Ok(got) => {
                let got_n = normalize(&got);
                if got_n == expected_n {
                    println!("✓ {} - verified (attempt {})", video_id, attempt);
                    return Ok(());
                }
                last_diff = Some((got_n.clone(), expected_n.clone()));
                println!(
                    "⏳ {} - verify mismatch on attempt {}/{}; retrying after 2s",
                    video_id, attempt, retries
                );
            }
            Err(e) => {
                println!(
                    "⏳ {} - verify GET failed on attempt {}/{}: {}; retrying after 2s",
                    video_id, attempt, retries, e
                );
            }
        }
        if attempt < retries {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    // All attempts failed — surface a concrete error so the caller treats this as failure.
    match last_diff {
        Some((got, exp)) => {
            // Truncate in the message; the diff itself can be large.
            let got_preview: String = got.chars().take(120).collect();
            let exp_preview: String = exp.chars().take(120).collect();
            anyhow::bail!(
                "verify failed for {}: YouTube echoed back different content after {} attempts. \
                 got[:120]={:?} expected[:120]={:?}",
                video_id,
                retries,
                got_preview,
                exp_preview
            );
        }
        None => anyhow::bail!(
            "verify failed for {}: could not re-fetch description after {} attempts",
            video_id,
            retries
        ),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();
    validate_profile_name(&cli.profile)?;
    println!("Using profile: {}", cli.profile);
    if cli.verify {
        println!("Verify: ON (retries: {})", cli.verify_retries);
    }
    if cli.only_if_different {
        println!("Only-if-different: ON (GET before PUT; skip if already canonical)");
    }

    if cli.args.len() < 2 {
        anyhow::bail!(
            "Usage: yt-set-description -p <PROFILE> <video_id> [<video_id>...] <content_file.txt>"
        );
    }

    let content_file = cli.args.last().unwrap().clone();
    let video_ids: Vec<String> = cli.args[..cli.args.len() - 1].to_vec();

    let content_path = Path::new(&content_file);
    if !content_path.exists() {
        anyhow::bail!("Content file not found: {}", content_file);
    }

    // Use raw content verbatim — no blank-line filtering (unlike append).
    let content = fs::read_to_string(&content_file)?;
    if content.trim().is_empty() {
        anyhow::bail!("Content file is empty: {}", content_file);
    }

    println!(
        "Overwriting description for {} video(s): {}",
        video_ids.len(),
        video_ids.join(", ")
    );
    println!("Content: {} bytes\n", content.len());

    let client = YouTubeClient::new(&cli.profile).await?;
    let mut ok = 0usize;
    let mut fail = 0usize;

    for video_id in &video_ids {
        // --only-if-different: GET live, compare to file; skip the write if equal.
        // This makes the tool a cheap canonical-enforcement check: no PUT (and no
        // rate-limit risk) when the description is already correct.
        if cli.only_if_different {
            match client.get_video_description(video_id).await {
                Ok(live) => {
                    if normalize(&live) == normalize(&content) {
                        println!("⊘ {} - already canonical, skip write", video_id);
                        ok += 1;
                        continue;
                    }
                    println!("↻ {} - live differs from canonical; overwriting", video_id);
                }
                Err(e) => {
                    // Couldn't pre-check (rate limit / transient). Proceed to SET so the
                    // caller still gets the write attempted; verify (if on) catches a miss.
                    println!(
                        "⏳ {} - pre-GET failed ({}); proceeding to write",
                        video_id, e
                    );
                }
            }
        }

        // SET, then verify. Both must succeed for this video to count as ok.
        let outcome = async {
            client.set_video_description(video_id, &content).await?;
            if cli.verify {
                verify_matches(&client, video_id, &content, cli.verify_retries).await?;
            }
            Ok::<(), anyhow::Error>(())
        };
        match outcome.await {
            Ok(()) => {
                println!(
                    "✓ {} - description set{}",
                    video_id,
                    if cli.verify { " + verified" } else { "" }
                );
                ok += 1;
            }
            Err(e) => {
                println!("✗ {} - error: {}", video_id, e);
                fail += 1;
            }
        }
    }

    println!("\n=== Summary ===\n✓ Success: {}\n✗ Failed:  {}", ok, fail);
    if fail > 0 {
        anyhow::bail!("{} video(s) failed", fail);
    }
    Ok(())
}
