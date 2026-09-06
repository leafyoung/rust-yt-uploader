import itertools
import pathlib
import subprocess
import sys

if __name__ == "__main__":
    # Define the input video files
    video_files = sys.argv[1::]

    if len(video_files) == 0:
        print("Please provide at least one video file as an argument.")
        sys.exit(1)

    for f in video_files:
        if not pathlib.Path(f).exists():
            print(f"File {f} does not exist. Please provide valid video files.")
            sys.exit(1)

    filter = "".join(
        [
            *[
                f"[{i}:v]scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30[v{i}]; "
                for i in range(1, len(video_files))
            ],
            *[f"[{i}:a]aformat=sample_rates=44100:channel_layouts=stereo[a{i}]; " for i in range(len(video_files))],
            *[f"[v{i}][a{i}]" for i in range(len(video_files))],
            f"concat=n={len(video_files)}:v=1:a=1[outv][outa]",
        ]
    )

    # Construct the ffmpeg command
    ffmpeg_command = [
        "ffmpeg",
        *itertools.chain.from_iterable(["-i", f] for f in video_files),  # Add input files
        "-filter_complex",
        filter,
        "-map",
        "[outv]",
        "-map",
        "[outa]",
        "-c:v",
        "libx264",
        "-crf",
        "18",
        "-c:a",
        "aac",
        "output.mp4",
    ]

    print(ffmpeg_command)

    # Execute the ffmpeg command
    subprocess.run(ffmpeg_command, check=True)
