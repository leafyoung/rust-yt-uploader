# AGENTS.md

Guidance for agentic coding agents working with Rust YouTube Uploader.

## Quick Start

**Build directory**: `~/.cache/rust-build/` (cached, never mention in docs)
**Edition**: Rust 2024 | **Error handling**: `anyhow::Result` | **Runtime**: Tokio async

### Essential Commands

```bash
# Build & check
cargo build --release --bin <name>    # Build specific binary
cargo check                           # Quick syntax check
cargo fmt                            # Auto-format code
cargo clippy --all-targets --all-features  # Lint

# Testing
cargo test --all-features            # Run all tests
cargo test test_name -- --nocapture  # Run specific test with output
cargo test --lib                     # Library tests only
```

## Project Structure

**Core Library** (`src/lib.rs`): `google_oauth`, `models`, `youtube_client`, `retry`, `progress_stream`, `video_process`

**CLI Binaries** (`src/bin/`): `yt-upload`, `yt-list`, `yt-append-description`, `yt-add-pin-comment`, `yt-add-tags`, `yt-update-lang`, `yt-update-date`, `yt-upload-subtitle`

**Tests**: `tests/integration.rs` (config parsing, validation); library tests in `src/**/*.rs`

## Code Style & Conventions

### Imports & Organization

- **Standard library first**, then external crates, then internal modules (separated by blank lines)
- Use glob imports sparingly; prefer explicit imports for clarity
- Re-export public types in `lib.rs` for API simplicity
- Use `use anyhow::Result` for all error-returning functions
- Trait objects: `Arc<dyn Trait>` for sharing (e.g., `Arc<dyn ProgressReporter>`)

### Naming & Types

- **Functions**: `snake_case`, methods starting with action verbs (`update_`, `post_`, `get_`)
- **Types**: `PascalCase` for structs/enums/traits
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Private helpers**: Prefix internal functions with underscore if needed
- **Type annotations**: Always explicit for public functions and struct fields

### Error Handling

- Use `anyhow::Result<T>` (not `std::result::Result`) everywhere
- Use `anyhow::bail!()` for quick errors: `anyhow::bail!("Error: {}", context)`
- Use `?` operator for propagation; wrap with `.map_err()` only if context needed
- Log errors with `error!()` before returning (uses tracing crate)
- Document retriable vs. non-retriable errors in comments

### Async & Concurrency

- **All operations are async**: Use `async fn` and `.await`
- Use `tokio::spawn()` for concurrent tasks; `Arc<Semaphore>` for rate limiting
- Document which fields/methods require `Send + Sync` traits
- Avoid blocking operations in async contexts

### Comments & Documentation

- **Public items**: Full doc comments `///` with examples and panics
- **Complex logic**: Explain the "why" not the "what"
- **Inline comments**: Use `//` for clarification, keep minimal
- **Tests**: Include usage examples in doc comments for public APIs

### Formatting

- 4-space indentation (enforced by `cargo fmt`)
- Max line length: ~100 characters (soft limit)
- One statement per line; use temporary variables for readability
- Format strings: Use `{}` interpolation, not `.to_string()` concatenation

### API Design

- Return `Result<T>` for fallible operations
- Trait bounds in function signatures: `<P: AsRef<Path>>` for flexibility
- Builder pattern for complex construction (use `with_*` methods)
- Private constructors with public factory methods (e.g., `YouTubeClient::new()`)

## Pre-commit Hooks

All commits run: `cargo fmt` → `cargo clippy` → `cargo test` → trailing whitespace checks

**To bypass** (never recommended): `git commit --no-verify`
**To fix formatting failures**: Run `cargo fmt` then re-stage files

## Key Patterns in Codebase

### Error Context

```rust
// ✅ Good: context before propagation
if !response.status().is_success() {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    return Err(anyhow!("Failed to X with status {}: {}", status, text));
}
```

### Async Patterns

```rust
// ✅ Good: explicit await, Arc for shared state
let client = Arc::new(YouTubeClient::new().await?);
let results = try_join_all(tasks).await?;
```

### Response Handling

```rust
// ✅ Good: deserialize then extract
#[derive(Deserialize)]
struct Response { items: Vec<Item> }
let resp: Response = response.json().await?;
```

## Versioning

**Version bumping**: Each commit increments minor version (`0.2.X` → `0.2.X+1`)
Update `Cargo.toml` version before committing; pre-commit hook validates changes.

## Important Constraints

- ❌ Never: `cargo clean` (breaks build cache)
- ❌ Never: Run binaries from `target/` directly (use `cargo run --bin`)
- ❌ Never: Commit `client_secret-{profile}.json`, `youtube-oauth2-{profile}.json`
- ✅ Always: Use `cargo test` before committing
- ✅ Always: Run `cargo fmt` on modified files
- ✅ Always: Test both sequential and concurrent upload modes if modifying `YouTubeClient`

## Testing Strategy

**Unit tests** (`#[test]`): Logic validation, mocking not needed
**Integration tests** (`tests/integration.rs`): Configuration parsing, file handling
**Ignored tests** (`#[ignore]`): Run explicitly with `cargo test -- --ignored`

Example: `cargo test test_batch_config -- --nocapture`
