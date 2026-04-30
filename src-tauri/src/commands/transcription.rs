use crate::cache;
use crate::error::{AppError, AppResult};
use crate::paths;
use crate::pipeline::{ffmpeg, harvester, whisper, whisper_openai, ytdlp};
use crate::state::{AppState, JobHandle};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State, Window};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeOptions {
    /// "local" (default) or "openai".
    #[serde(default)]
    pub backend: Option<String>,
    /// Required when backend == "openai".
    #[serde(default)]
    pub openai_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscribeResult {
    pub video_id: String,
    pub title: Option<String>,
    pub audio_path: String,
    pub clips: Vec<Clip>,
    pub from_cache: bool,
}

#[derive(Serialize, Clone)]
struct ProgressPayload<'a> {
    stage: &'a str,
    pct: f32,
    message: Option<String>,
}

fn emit(window: &Window, stage: &str, pct: f32, msg: Option<String>) {
    let _ = window.emit(
        "transcription-progress",
        ProgressPayload { stage, pct, message: msg },
    );
}

/// Extract an 11-char YouTube video id from a URL. Falls back to an error
/// rather than guessing — Phase 2+ will add a SHA1 fallback for exotic URLs.
fn parse_video_id(raw: &str) -> AppResult<String> {
    let url = url::Url::parse(raw).map_err(|_| AppError::InvalidUrl)?;
    let host = url.host_str().unwrap_or("").to_lowercase();

    // youtu.be/VIDEO_ID
    if host.ends_with("youtu.be") {
        if let Some(seg) = url.path_segments().and_then(|mut s| s.next()) {
            if is_valid_id(seg) {
                return Ok(seg.to_string());
            }
        }
    }

    // youtube.com/watch?v=VIDEO_ID
    if host.contains("youtube.com") {
        if let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "v") {
            if is_valid_id(&v) {
                return Ok(v.into_owned());
            }
        }
        // youtube.com/shorts/VIDEO_ID  or  /embed/VIDEO_ID
        if let Some(segs) = url.path_segments() {
            let parts: Vec<&str> = segs.collect();
            if let Some(idx) = parts.iter().position(|s| matches!(*s, "shorts" | "embed" | "v")) {
                if let Some(id) = parts.get(idx + 1) {
                    if is_valid_id(id) {
                        return Ok((*id).to_string());
                    }
                }
            }
        }
    }

    Err(AppError::InvalidUrl)
}

fn is_valid_id(s: &str) -> bool {
    s.len() == 11 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[tauri::command]
pub async fn start_transcription(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    url: String,
    options: Option<TranscribeOptions>,
) -> AppResult<TranscribeResult> {
    let options = options.unwrap_or_default();
    let backend = options.backend.as_deref().unwrap_or("local");
    let video_id = parse_video_id(&url)?;

    // Cache check first — if we already transcribed this video, skip the
    // whole pipeline.
    if let Some(cached) = cache::read(&app, &video_id)? {
        emit(&window, "cache", 1.0, Some("Loaded from cache".into()));
        return Ok(cached);
    }

    // Cancel any prior job — handles paste-while-running.
    {
        let mut guard = state.current_job.lock().await;
        if let Some(prev) = guard.take() {
            prev.cancel.cancel();
        }
    }

    let cancel = CancellationToken::new();
    {
        let mut guard = state.current_job.lock().await;
        *guard = Some(JobHandle {
            video_id: video_id.clone(),
            cancel: cancel.clone(),
        });
    }

    // Phase 1: download only. ffmpeg/whisper land in Phase 2.
    let window_for_progress = window.clone();
    let on_progress: Arc<dyn Fn(f32, &str) + Send + Sync> =
        Arc::new(move |pct: f32, line: &str| {
            if pct >= 0.0 {
                emit(&window_for_progress, "download", pct, None);
            } else {
                emit(
                    &window_for_progress,
                    "download",
                    0.0,
                    Some(line.trim().to_string()),
                );
            }
        });

    // Stage 1: download.
    emit(&window, "download", 0.0, Some("Starting yt-dlp…".into()));
    let dl =
        ytdlp::download_audio(&app, &url, &video_id, on_progress, cancel.clone()).await?;
    let audio_path = dl.audio_path;
    let title = dl.title;
    emit(&window, "download", 1.0, Some("Download complete.".into()));

    // Stages 2 + 3: convert + transcribe. The OpenAI backend skips the WAV
    // conversion entirely — it accepts the raw m4a directly, which also
    // saves an upload of the larger PCM file.
    let window_for_whisper = window.clone();
    let on_whisper: Arc<dyn Fn(f32, &str) + Send + Sync> =
        Arc::new(move |pct: f32, _line: &str| {
            emit(&window_for_whisper, "transcribe", pct, None);
        });

    let (segments, words) = match backend {
        "openai" => {
            // Prefer the key the user typed in Settings; fall back to the
            // dev .env (`OPENAI_API_KEY`) so we don't have to paste it during
            // local development.
            let key = options
                .openai_api_key
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .unwrap_or_default();
            if key.is_empty() {
                return Err(AppError::Other(
                    "Add your OpenAI API key in Settings to use the OpenAI backend.".into(),
                ));
            }
            emit(&window, "transcribe", 0.0, Some("Calling OpenAI…".into()));
            whisper_openai::transcribe(&audio_path, &key, on_whisper, cancel.clone()).await?
        }
        _ => {
            // Local whisper.cpp path.
            let wav_path = audio_path.with_extension("wav");
            emit(&window, "convert", 0.0, Some("Converting to WAV…".into()));
            ffmpeg::to_wav_16k_mono(&app, &audio_path, &wav_path, cancel.clone()).await?;
            emit(&window, "convert", 1.0, None);

            let model = paths::model_path(&app)?;
            tracing::info!("whisper model = {}", model.display());
            emit(&window, "transcribe", 0.0, Some("Loading model…".into()));
            let result =
                whisper::transcribe(&app, &wav_path, &model, on_whisper, cancel.clone()).await;

            // Always clean up the intermediate WAV, even on error.
            let _ = tokio::fs::remove_file(&wav_path).await;
            result?
        }
    };
    emit(&window, "transcribe", 1.0, None);

    tracing::info!(
        "whisper produced {} segments, {} words",
        segments.len(),
        words.len()
    );

    // Stage 4: harvest sentence-shaped clips for shadowing.
    emit(&window, "harvest", 0.0, Some("Harvesting clips…".into()));
    let clips = harvester::harvest(&segments, &words);
    emit(&window, "harvest", 1.0, None);
    tracing::info!("harvested {} clips", clips.len());
    if let Some(c) = clips.first() {
        tracing::info!(
            "first clip: [{}ms..{}ms] {:?}",
            c.start_ms, c.end_ms, c.text
        );
    }

    // Clear our slot when we finish (cancellation-safe — only clear if we're
    // still the current job).
    {
        let mut guard = state.current_job.lock().await;
        if let Some(cur) = guard.as_ref() {
            if cur.video_id == video_id {
                *guard = None;
            }
        }
    }

    let result = TranscribeResult {
        video_id,
        title,
        audio_path: audio_path.to_string_lossy().into_owned(),
        clips,
        from_cache: false,
    };
    if let Err(e) = cache::write(&app, &result) {
        tracing::warn!("cache write failed: {e}");
    }
    Ok(result)
}

#[tauri::command]
pub async fn list_videos(app: AppHandle) -> AppResult<Vec<cache::VideoSummary>> {
    cache::list(&app)
}

#[tauri::command]
pub async fn load_cached(
    app: AppHandle,
    video_id: String,
) -> AppResult<Option<TranscribeResult>> {
    cache::read(&app, &video_id)
}

#[tauri::command]
pub async fn delete_cached(app: AppHandle, video_id: String) -> AppResult<()> {
    let cache_path = paths::cache_dir(&app)?.join(format!("{video_id}.json"));
    let _ = std::fs::remove_file(&cache_path);
    // Best-effort: also drop the audio file.
    let audio_dir = paths::audio_dir(&app)?;
    if let Ok(read) = std::fs::read_dir(&audio_dir) {
        for e in read.flatten() {
            let p = e.path();
            if p.file_stem().and_then(|s| s.to_str()) == Some(&video_id) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cancel_transcription(state: State<'_, AppState>) -> AppResult<()> {
    let mut guard = state.current_job.lock().await;
    if let Some(job) = guard.take() {
        job.cancel.cancel();
    }
    Ok(())
}
