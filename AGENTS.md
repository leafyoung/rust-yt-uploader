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

### ⚠️ CRITICAL: Never use `--no-verify`

**❌ NEVER use `git commit --no-verify`** - This bypasses all pre-commit checks and will likely cause CI failures.

If you used `--no-verify` and CI fails:
1. Fix the issues (usually formatting: run `cargo fmt`)
2. Re-stage files: `git add .`
3. Commit **without** `--no-verify`: `git commit -m "fix: ..."`
4. Push and verify CI passes

### Common Pre-commit Failures

| Failure | Cause | Fix |
|---------|-------|-----|
| `cargo fmt` | Code formatting | Run `cargo fmt` then re-stage |
| `cargo clippy` | Lint warnings | Fix warnings or run `cargo clippy --fix` |
| `cargo test` | Test failures | Fix failing tests |

### Post-Commit CI Verification Checklist

After pushing any commit, agents MUST verify CI passes:

- [ ] Push commit and note the run ID
- [ ] Run `gh run watch --repo leafyoung/rust-yt-uploader --exit-status` until completion
- [ ] If CI fails, run `gh run view --log-failed --repo leafyoung/rust-yt-uploader` to diagnose
- [ ] Fix any issues and re-push until all jobs pass
- [ ] Only proceed to next task after CI is green

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

## GitHub CI Verification

After pushing commits, always verify CI passes using `gh` CLI (not direct links):

```bash
# View CI run status (get run ID from push output or list)
gh run list --repo leafyoung/rust-yt-uploader --limit 3

# Watch CI run progress with live updates
gh run watch <run-id> --repo leafyoung/rust-yt-uploader --exit-status

# If CI fails, view detailed failure logs
gh run view <run-id> --repo leafyoung/rust-yt-uploader --log-failed

# View specific job logs
gh run view <run-id> --repo leafyoung/rust-yt-uploader --job <job-id>
```

### Common CI Issues and Fixes

1. **Rust version too old**: Dependencies may require newer Rust. Update `rust-version` in `Cargo.toml` and CI matrix.
2. **Mold linker not available**: CI runners don't have mold installed. Use `rui314/setup-mold@v1` action in workflow.
3. **Security vulnerabilities**: `cargo audit` may find issues. Update vulnerable dependencies via `cargo update`.
4. **Node.js deprecation warnings**: Use newer action versions (e.g., `Swatinem/rust-cache@v2` instead of `actions/cache@v3`).

### Workflow Example

```bash
# 1. Make changes and commit
git add . && git commit -m "fix: something"

# 2. Push and note the run ID from output
git push

# 3. Watch CI until completion
gh run watch --repo leafyoung/rust-yt-uploader --exit-status

# 4. If failed, diagnose and fix
gh run view --log-failed --repo leafyoung/rust-yt-uploader
```

## Runtime Audit & Live Test Cases

Adjudicated results live in `review_reports/` (untracked): `IMPROVEMENT_REPORT.md` (read this
first), `demo_runs.csv` (per-command measurements), `demo_logs/` (raw output).

### Harness sweep (in box2 container)

```bash
podman exec box2 bash -lc 'cd /var/home/yangye/devv/dongli/uploader/rust-yt-uploader && python3 measure_rust.py'           # RSS + wall, debug/release
podman exec box2 bash -lc 'cd /var/home/yangye/devv/dongli/uploader/rust-yt-uploader && python3 measure_rust.py --massif'  # heap lines
podman exec box2 bash -lc 'cd /var/home/yangye/devv/dongli/uploader/rust-yt-uploader && python3 measure_rust.py --leaks'   # memcheck
```

- Binaries exit 2 on no-args (expected clap usage error) — that is the only runnable path without
  credentials. `RUN_FAIL` in CSVs means this, not a defect.
- Resumable-skip trap: stale CSVs make `--massif`/`--leaks` silently skip all units. Delete the
  CSV first to force a fresh sweep.
- Adjudication standing rule: "possibly lost 11,856 B + 2 errors" = tokio teardown noise
  (process::exit skips runtime Drop), NOT a leak. Real-path (normal return) runs are 0/0/0.

### Live demo test cases (measured via `python3 measure_cmd.py <label> <cmd...>`, host)

Safe to run repeatedly — all are read-only or idempotent by design:

```bash
BIN=$HOME/.cache/rust-build/release
$BIN/yt-list --format jsonl --profile dongli
$BIN/yt-update-lang --dry-run --profile dongli
ID=ZrTvGlp87jo  # any video
# idempotent description PUT: write the video's CURRENT description back (full GET+PUT path)
$BIN/yt-set-description $ID <file-with-current-description> --profile dongli
# idempotent date write: JSON [{"id","title","guessed_date"}] with the EXISTING recordingDate
$BIN/yt-update-date <json-file> --profile dongli
```

Single-shot (mutate or create resources — run at most once per invocation unless intended):

```bash
$BIN/yt-upload --file /var/home/yangye/devv/dongli/uploader/ups/2026/video_20260329_test.yaml --profile dongli
$BIN/yt-upload --file <same.yaml> --async --concurrent 3 --profile dongli
$BIN/yt-upload-subtitle --video-id $ID --srt-file <srt> --language zh --profile dongli
$BIN/yt-append-description $ID <meta.txt> --profile dongli   # built-in dupe check
$BIN/yt-add-tags $ID <tags.txt> --profile dongli
$BIN/yt-add-pin-comment $ID <chapters.txt> --pin --profile dongli  # skips if already pinned
```

Safety properties: the test YAML has `test: true` (uploads then auto-deletes the private videos)
and `privacyStatus: private`. Never run mutating commands under valgrind/TSan twice for
measurement + detector separately — pick one (a second run would double-post).

### Quotas and TSan

- `yt-list` and `yt-update-lang` consume `search.list` quota: **100/day/project, resets midnight
  PT**. rc=1 + quota JSON in output = quota exhausted, not a bug.
- TSan on the concurrent path (`--async` upload) works in box2:
  `rustup component add rust-src --toolchain nightly`, then build with
  `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-Zsanitizer=thread -C debuginfo=2"` +
  `--target x86_64-unknown-linux-gnu` + `-Z build-std=std` (target-scoped flags + `--target` are
  REQUIRED — plain `RUSTFLAGS` breaks build scripts with ABI-mismatch errors). Expect exit 66
  with ~2 tokio-internal `ScheduledIo` warnings (loom-verified pattern, known TSan blind spot);
  anything in `youtube_client.rs`/`progress_stream.rs` racing accesses would be a real finding.
