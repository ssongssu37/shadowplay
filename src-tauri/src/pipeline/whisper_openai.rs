use crate::error::{AppError, AppResult};
use crate::pipeline::whisper::{WhisperSegment, WhisperWord};
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";

/// Transcribe via OpenAI's `whisper-1` API.
///
/// `verbose_json` + `timestamp_granularities[]=word,segment` returns both
/// arrays we need with millisecond-level precision (returned as seconds).
///
/// We send the .m4a directly — the API accepts m4a, mp3, mp4, mpeg, mpga,
/// wav, and webm up to 25 MB. yt-dlp's bestaudio is typically well under
/// that for clips we care about. (For >25 MB we'd need to re-encode to a
/// lower bitrate or chunk, but that's out of scope for Phase 2.)
pub async fn transcribe(
    audio: &Path,
    api_key: &str,
    on_progress: Arc<dyn Fn(f32, &str) + Send + Sync>,
    cancel: CancellationToken,
) -> AppResult<(Vec<WhisperSegment>, Vec<WhisperWord>)> {
    if api_key.trim().is_empty() {
        return Err(AppError::Other("OpenAI API key is empty".into()));
    }
    let bytes = tokio::fs::read(audio).await?;
    let filename = audio
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio.m4a")
        .to_string();

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str("application/octet-stream")
        .map_err(|e| AppError::Other(e.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", "whisper-1")
        .text("response_format", "verbose_json")
        .text("timestamp_granularities[]", "word")
        .text("timestamp_granularities[]", "segment");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;

    on_progress(0.05, "Uploading to OpenAI…");

    let req = client
        .post(ENDPOINT)
        .bearer_auth(api_key)
        .multipart(form)
        .send();

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
        r = req => r.map_err(|e| AppError::Other(format!("openai network: {e}")))?,
    };

    let status = resp.status();
    let body = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
        b = resp.bytes() => b.map_err(|e| AppError::Other(e.to_string()))?,
    };
    if !status.is_success() {
        let snippet = String::from_utf8_lossy(&body);
        return Err(AppError::Whisper(format!(
            "openai http {}: {}",
            status,
            snippet.chars().take(400).collect::<String>()
        )));
    }
    on_progress(0.95, "Parsing response…");
    let parsed: VerboseJson = serde_json::from_slice(&body)?;

    let segments = parsed
        .segments
        .unwrap_or_default()
        .into_iter()
        .map(|s| WhisperSegment {
            text: s.text.trim().to_string(),
            start: s.start,
            end: s.end,
        })
        .collect();
    let words = parsed
        .words
        .unwrap_or_default()
        .into_iter()
        .map(|w| WhisperWord {
            word: w.word,
            start: w.start,
            end: w.end,
        })
        .collect();

    on_progress(1.0, "Done.");
    Ok((segments, words))
}

#[derive(Debug, Deserialize)]
struct VerboseJson {
    #[serde(default)]
    segments: Option<Vec<RawSegment>>,
    #[serde(default)]
    words: Option<Vec<RawWord>>,
}

#[derive(Debug, Deserialize)]
struct RawSegment {
    text: String,
    start: f64,
    end: f64,
}

#[derive(Debug, Deserialize)]
struct RawWord {
    word: String,
    start: f64,
    end: f64,
}
