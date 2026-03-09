# YouTube Uploader shortcuts

# Upload a video with the given config file
# Usage: just upload ../ups/2026/video_20260308.yaml
upload file:
    cargo run --release --bin yt-upload -- --file {{file}}
