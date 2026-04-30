# ShadowPlay

A desktop shadowing player for ESL learners. Paste a YouTube URL — the app
breaks the audio into ~5-second sentence clips and plays them one at a time
with configurable repeats and a gap between each play, so you can repeat
aloud after the speaker.

Built with Tauri 2 + React + Rust. Local transcription via `whisper.cpp`,
or fast cloud transcription via the OpenAI Whisper API (your key stays
local).

## Download

Grab a prebuilt installer from
[Releases](https://github.com/ssongssu37/shadowplay/releases) — Windows
`.msi` and macOS `.dmg`.

## Build from source

```sh
git clone https://github.com/ssongssu37/shadowplay
cd shadowplay
npm install
scripts/fetch-binaries.sh        # downloads yt-dlp, ffmpeg, whisper-cli
curl -L -o src-tauri/resources/ggml-tiny.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin
npm run tauri dev
```

Requires: Rust (stable), Node 20+, and on macOS `brew install ffmpeg whisper-cpp`.

## How it works

1. **Download** — `yt-dlp` pulls bestaudio from YouTube as `.m4a`.
2. **Transcribe** — either local `whisper.cpp` (free, offline) or OpenAI
   Whisper API (~10s for a 5-min video, $0.006/min).
3. **Harvest** — sentence-shaped clips (≤20 words, ≤5s) are extracted from
   the word-level timestamps, snapped to actual word boundaries.
4. **Play** — clip 1 → gap → clip 1 (repeat) → gap → clip 2 → … with
   adjustable repeat count and gap mode.

## License

MIT.
