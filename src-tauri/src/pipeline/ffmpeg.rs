use crate::error::{AppError, AppResult};
use crate::sidecar::spawn_streaming;
use std::path::Path;
use std::sync::Arc;
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

/// Decode `src` (m4a/webm/opus/whatever yt-dlp produced) into a 16 kHz mono
/// little-endian s16 PCM WAV at `dst`. This is whisper.cpp's required input
/// format. ffmpeg picks the demuxer and decoder automatically.
pub async fn to_wav_16k_mono(
    app: &AppHandle,
    src: &Path,
    dst: &Path,
    cancel: CancellationToken,
) -> AppResult<()> {
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        src.to_string_lossy().into_owned(),
        "-vn".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-y".into(),
        dst.to_string_lossy().into_owned(),
    ];

    let on_line: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|line: &str| {
        let l = line.trim();
        if !l.is_empty() {
            tracing::warn!("ffmpeg: {}", l);
        }
    });

    let code = spawn_streaming(app, "binaries/ffmpeg", args, on_line, cancel).await?;
    if code != 0 {
        return Err(AppError::FFmpeg(format!("exit {code}")));
    }
    if !dst.exists() {
        return Err(AppError::FFmpeg("output wav not produced".into()));
    }
    Ok(())
}
