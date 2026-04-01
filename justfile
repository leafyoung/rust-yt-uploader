# YouTube Uploader shortcuts

# Upload a video with the given config file
# Usage: just upload video_20260308.yaml
upload file profile:
    cargo run --release --bin yt-upload -- --file {{file}} -p {{profile}}
