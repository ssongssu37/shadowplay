# Sidecar binaries

This directory holds platform-specific copies of `yt-dlp`, `ffmpeg`, and
`whisper-cli`, named with the Rust target triple suffix Tauri expects:

```
yt-dlp-<target>[.exe]
ffmpeg-<target>[.exe]
whisper-cli-<target>[.exe]
```

These files are **not committed** — fetch them locally with:

```
scripts/fetch-binaries.sh
```

CI fetches them per-platform during release builds; see
[.github/workflows/build.yml](../../.github/workflows/build.yml).
