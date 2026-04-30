//! Per-video transcript cache.
//!
//! After a successful pipeline run we serialize the full `TranscribeResult`
//! to `<cache_dir>/<video_id>.json`. On the next paste of the same URL (or
//! a click in the Library list) we read the JSON back instead of running
//! yt-dlp + ffmpeg + whisper again.
//!
//! The audio file lives at `<audio_dir>/<video_id>.m4a` and is referenced
//! by absolute path in the cached entry. If the audio file has been deleted
//! out-of-band, the cache entry is treated as stale.

use crate::commands::transcription::TranscribeResult;
use crate::error::AppResult;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub schema_v: u32,
    pub video_id: String,
    pub title: Option<String>,
    pub audio_path: String,
    pub clips: Vec<crate::commands::transcription::Clip>,
    pub fetched_at: i64, // unix seconds
}

const SCHEMA: u32 = 1;

fn entry_path(app: &AppHandle, video_id: &str) -> AppResult<PathBuf> {
    Ok(paths::cache_dir(app)?.join(format!("{video_id}.json")))
}

pub fn read(app: &AppHandle, video_id: &str) -> AppResult<Option<TranscribeResult>> {
    let path = entry_path(app, video_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    let entry: CacheEntry = match serde_json::from_str(&raw) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("cache parse failed for {video_id}: {e}; ignoring");
            return Ok(None);
        }
    };
    if entry.schema_v != SCHEMA {
        return Ok(None);
    }
    if !PathBuf::from(&entry.audio_path).exists() {
        // The audio file was deleted; force a re-download.
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }
    Ok(Some(TranscribeResult {
        video_id: entry.video_id,
        title: entry.title,
        audio_path: entry.audio_path,
        clips: entry.clips,
        from_cache: true,
    }))
}

pub fn write(app: &AppHandle, result: &TranscribeResult) -> AppResult<()> {
    let path = entry_path(app, &result.video_id)?;
    let entry = CacheEntry {
        schema_v: SCHEMA,
        video_id: result.video_id.clone(),
        title: result.title.clone(),
        audio_path: result.audio_path.clone(),
        clips: result.clips.clone(),
        fetched_at: now_unix(),
    };
    let json = serde_json::to_vec_pretty(&entry)?;
    std::fs::write(&path, json)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoSummary {
    pub video_id: String,
    pub title: Option<String>,
    pub clip_count: usize,
    pub fetched_at: i64,
    pub audio_path: String,
}

pub fn list(app: &AppHandle) -> AppResult<Vec<VideoSummary>> {
    let dir = paths::cache_dir(app)?;
    let mut entries: Vec<VideoSummary> = Vec::new();
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return Ok(entries),
    };
    for e in read.flatten() {
        let p = e.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = match std::fs::read_to_string(&p) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let entry: CacheEntry = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.schema_v != SCHEMA {
            continue;
        }
        if !PathBuf::from(&entry.audio_path).exists() {
            continue;
        }
        entries.push(VideoSummary {
            video_id: entry.video_id,
            title: entry.title,
            clip_count: entry.clips.len(),
            fetched_at: entry.fetched_at,
            audio_path: entry.audio_path,
        });
    }
    // Newest first.
    entries.sort_by(|a, b| b.fetched_at.cmp(&a.fetched_at));
    Ok(entries)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
