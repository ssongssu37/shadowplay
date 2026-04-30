use crate::error::{AppError, AppResult};
use crate::paths;
use crate::sidecar::spawn_streaming;
use regex::Regex;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

/// Download bestaudio from a YouTube URL into `audio_dir/<video_id>.m4a`.
///
/// `--remux-video m4a` forces an m4a container without re-encoding when
/// the source is AAC (which it is for most YouTube audio streams). This
/// avoids `.opus` files that WebView2 can't play on Windows.
///
/// Progress is parsed from yt-dlp's `[download]  37.4% of …` lines emitted
/// when we pass `--newline`. We pass each parsed value (0.0..1.0) to
/// `on_progress`.
pub struct DownloadResult {
    pub audio_path: PathBuf,
    pub title: Option<String>,
}

pub async fn download_audio(
    app: &AppHandle,
    url: &str,
    video_id: &str,
    on_progress: Arc<dyn Fn(f32, &str) + Send + Sync>,
    cancel: CancellationToken,
) -> AppResult<DownloadResult> {
    let dir = paths::audio_dir(app)?;
    tracing::info!("audio_dir = {}", dir.display());
    let out_template = dir.join(format!("{}.%(ext)s", video_id));

    let args: Vec<String> = vec![
        // --print emits the title to stdout BEFORE the download starts, so
        // we can capture it without a second yt-dlp call. The format:
        // BEFORE_DL: %(title)s — newline-terminated.
        "--print".into(),
        "before_dl:TITLE\t%(title)s".into(),
        "-f".into(),
        "bestaudio[ext=m4a]/bestaudio".into(),
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--newline".into(),
        "--no-part".into(),
        "--remux-video".into(),
        "m4a".into(),
        "-o".into(),
        out_template.to_string_lossy().into_owned(),
        url.into(),
    ];

    let pct_re = Arc::new(Regex::new(r"\[download\]\s+([\d.]+)%").unwrap());
    let pct_re_cb = pct_re.clone();
    let on_progress_cb = on_progress.clone();
    let title_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let title_capture = title_slot.clone();

    let on_line: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line: &str| {
        if let Some(rest) = line.strip_prefix("TITLE\t") {
            let t = rest.trim().to_string();
            if !t.is_empty() {
                if let Ok(mut slot) = title_capture.lock() {
                    *slot = Some(t);
                }
            }
            return;
        }
        if let Some(caps) = pct_re_cb.captures(line) {
            if let Ok(p) = caps[1].parse::<f32>() {
                on_progress_cb(p / 100.0, line);
            }
        } else if !line.trim().is_empty() {
            on_progress_cb(-1.0, line);
        }
    });

    let code = spawn_streaming(app, "binaries/yt-dlp", args, on_line, cancel).await?;
    if code != 0 {
        return Err(AppError::YtDlp(format!("exit {code}")));
    }

    // yt-dlp may produce .m4a, .webm, etc. depending on what it could remux.
    // Glob the output directory for files starting with our video_id.
    let mut found: Option<PathBuf> = None;
    let mut entries = tokio::fs::read_dir(&dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let p = entry.path();
        if p.file_stem().and_then(|s| s.to_str()) == Some(video_id) {
            // Skip the tiny .info.json or .description sidecar files yt-dlp
            // sometimes emits.
            if matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("m4a") | Some("mp4") | Some("webm") | Some("opus") | Some("mp3")
            ) {
                found = Some(p);
                break;
            }
        }
    }
    let audio_path = found
        .ok_or_else(|| AppError::YtDlp("output file not found after download".into()))?;
    let title = title_slot.lock().ok().and_then(|s| s.clone());
    Ok(DownloadResult { audio_path, title })
}
