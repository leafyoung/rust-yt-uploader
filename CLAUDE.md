# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Rust YouTube Uploader is a high-performance YouTube video uploader library with CLI tools. It uses OAuth 2.0 (PKCE flow) for authentication and supports both sequential and concurrent uploads with retry logic and progress tracking.

**Build directory**: `~/.cache/rust-build/` (never mention this in documentation or code).

**Key Design**: Async-first architecture using Tokio runtime. All operations are async and the library uses `anyhow` for error handling.

## Development Commands

### Building

```bash
# Build specific binaries (never run from target/ directly)
cargo build --release --bin yt-upload
cargo build --release --bin yt-list
cargo build --release --bin yt-update-lang

# Quick check (faster than full build)
cargo check
```

### Testing

```bash
# Run all tests
cargo test --all-features

# Run specific test
cargo test test_name

# Run tests with output
cargo test --all-features -- --nocapture
```

### Code Quality

```bash
# Format code
cargo fmt

# Check formatting without changes
cargo fmt -- --check

# Run linter
cargo clippy --all-targets --all-features

# Pre-commit hooks (configured in .pre-commit-config.yaml)
pre-commit install          # Install git hooks
pre-commit run --all-files  # Run manually
pre-commit autoupdate       # Update hooks to latest versions
```

## Architecture

### Module Structure

**Core Library** (`src/lib.rs`):

- `google_oauth/`: OAuth 2.0 authentication with PKCE support
  - `credentials.rs`: Client secret loading
  - `google.rs`: `GoogleOAuth` main client (token management, auto-refresh)
  - `oauth.rs`: Interactive OAuth flow with local HTTP server for code exchange
- `models.rs`: Configuration models with validation (individual/batch YAML formats)
- `youtube_client.rs`: Core upload logic, progress reporting, playlist management
- `retry.rs`: Exponential backoff with jitter for retriable HTTP errors
- `progress_stream.rs`: Stream wrapper for progress tracking + bandwidth throttling
- `video_process.rs`: FFmpeg integration for merging multiple video files

**CLI Binaries** (`src/bin/`):

- `yt-upload`: Main uploader (sequential/concurrent modes)
- `yt-list`: List/export channel videos (table/JSON/JSONL formats)
- `yt-update-lang`: Update language metadata for videos
- `yt-update-date`: Update recording date metadata
- `yt-update-description`: Update video descriptions
- `yt-upload-subtitle`: Upload caption/subtitle files
- `yt-add-tags`: Add tags to existing videos

### Key Patterns

**OAuth Flow**:

1. First run: Interactive flow displays auth URL → opens browser → user pastes code → tokens saved to `youtube-oauth2.json`
2. Subsequent runs: Loads existing tokens, auto-refreshes on expiry
3. Credentials files (`client_secret.json`, token files) are **never** committed to git

**Configuration Formats**:

- **Batch format**: `common` section + separate `titles` and `files` arrays (recommended)
- **Individual format**: `videos` array with per-video configuration
- Both support comma/semicolon/space-separated file lists for merging multiple videos

**Upload Modes**:

- **Sequential**: One video at a time with progress bars (default)
- **Concurrent**: Configurable concurrency (default 3), uses `Semaphore` for throttling

**Error Handling**:

- Library uses `anyhow::Result` for error propagation
- Retriable errors: HTTP 500-504, connection errors, timeouts, IO errors
- Non-retriable: Client errors (4xx), validation failures

**Progress Tracking**:

- `ProgressReporter` trait with implementations: `NoProgress`, `ProgressBarReporter`
- `ProgressStream` wraps upload streams for real-time tracking

### File Handling

**Video Merging**:

- File entries can contain multiple paths separated by `,` or `;`
- Automatically calls `merge_videos_with_ffmpeg()` to concatenate before upload
- Uses concat demuxer (no re-encoding, fast operation)

**MIME Types**:

- Uses `mime_guess` for auto-detection
- Special handling for `.mts` files → `video/mp2t`

## Versioning

**Version Bumping**: Each commit shall bump the minor version in `Cargo.toml`. For example:

- After committing, the version changes from `0.2.9` → `0.2.10` → `0.2.11`, etc.
- This automatic versioning is managed during the commit process
- Patch version increments only; major/minor versions remain stable unless explicitly changed

## Important Constraints

- **Never run `cargo clean`** (build directory is cached)
- **Never run binaries** from `target/debug/` or `target/release/` directly
- Always use `cargo run --bin <name>` for execution
- Rust 2024 edition required
- Build directory location is an implementation detail - never expose it in docs/code

## Test

Integration tests are in `tests/integration.rs`. They test:

- Configuration parsing (individual/batch formats)
- Validation logic (playlist IDs, categories, privacy status)
- File format detection

Tests with `#[ignore]` are meant for explicit running (e.g., retry logic tests with delays).
