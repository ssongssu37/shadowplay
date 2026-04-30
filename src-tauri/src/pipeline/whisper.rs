use crate::error::{AppError, AppResult};
use crate::sidecar::spawn_streaming;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

// ----- public output types ---------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperWord {
    pub word: String,
    pub start: f64, // seconds
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

// ----- raw whisper-cli JSON shapes ------------------------------------------

#[derive(Debug, Deserialize)]
struct WhisperJson {
    transcription: Vec<RawSegment>,
}

#[derive(Debug, Deserialize)]
struct RawSegment {
    text: String,
    offsets: Offsets,
    #[serde(default)]
    tokens: Vec<RawToken>,
}

#[derive(Debug, Deserialize)]
struct RawToken {
    text: String,
    offsets: Offsets,
}

#[derive(Debug, Deserialize)]
struct Offsets {
    from: i64,
    to: i64,
}

// ----- main entry point -----------------------------------------------------

/// Run whisper-cli on a 16kHz mono WAV, parse `--output-json-full` output,
/// and return parallel segment + word arrays.
///
/// The model file is passed as an absolute path so the runtime doesn't depend
/// on whisper-cli's cwd.
pub async fn transcribe(
    app: &AppHandle,
    wav: &Path,
    model: &Path,
    on_progress: Arc<dyn Fn(f32, &str) + Send + Sync>,
    cancel: CancellationToken,
) -> AppResult<(Vec<WhisperSegment>, Vec<WhisperWord>)> {
    if !model.exists() {
        return Err(AppError::ModelMissing);
    }

    // Output goes to <wav stem>.json next to the wav. -of takes the prefix
    // (without extension); whisper-cli appends `.json`.
    let out_prefix = wav.with_extension("");
    let out_json = wav.with_extension("json");
    // Stale output from a prior run would mask a hard failure where whisper
    // exits 0 but writes nothing — clear it first.
    let _ = std::fs::remove_file(&out_json);

    // Threads: cap at 8. Whisper.cpp scales sublinearly past that.
    let threads = std::cmp::min(num_threads(), 8).to_string();

    let args: Vec<String> = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-l".into(),
        "en".into(),
        "-t".into(),
        threads,
        "-pp".into(),
        "-ojf".into(),
        "-of".into(),
        out_prefix.to_string_lossy().into_owned(),
    ];

    let pct_re = Arc::new(Regex::new(r"progress\s*=\s*(\d+)%").unwrap());
    let pct_re_cb = pct_re.clone();
    let on_progress_cb = on_progress.clone();
    let on_line: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(move |line: &str| {
        if let Some(caps) = pct_re_cb.captures(line) {
            if let Ok(p) = caps[1].parse::<f32>() {
                on_progress_cb(p / 100.0, line);
                return;
            }
        }
        let l = line.trim();
        if !l.is_empty() && !l.starts_with("whisper_") {
            tracing::debug!("whisper: {}", l);
        }
    });

    let code = spawn_streaming(app, "binaries/whisper-cli", args, on_line, cancel).await?;
    if code != 0 {
        return Err(AppError::Whisper(format!("exit {code}")));
    }
    if !out_json.exists() {
        return Err(AppError::Whisper(format!(
            "expected JSON not found at {}",
            out_json.display()
        )));
    }

    let raw = std::fs::read_to_string(&out_json)?;
    let parsed: WhisperJson = serde_json::from_str(&raw)?;
    // Best-effort cleanup; not critical if it fails.
    let _ = std::fs::remove_file(&out_json);

    let mut segments = Vec::with_capacity(parsed.transcription.len());
    let mut words = Vec::new();

    for seg in &parsed.transcription {
        let text = seg.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        segments.push(WhisperSegment {
            text,
            start: ms_to_s(seg.offsets.from),
            end: ms_to_s(seg.offsets.to),
        });
        merge_tokens_into(&seg.tokens, &mut words);
    }

    Ok((segments, words))
}

// ----- helpers --------------------------------------------------------------

fn ms_to_s(ms: i64) -> f64 {
    ms.max(0) as f64 / 1000.0
}

fn num_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

/// Merge whisper.cpp BPE subword tokens into whole words.
///
/// Rule, mirroring SentenceHarvester's expectations:
///   - Skip special tokens (text starts with `[_`, e.g. `[_BEG_]`).
///   - A token whose text starts with a space begins a new word.
///   - Otherwise (no leading space, e.g. punctuation `,` or subword piece
///     `ing`), append to the previous word and extend its `end`.
///   - If `out` is empty, the first non-special token always starts a new
///     word, even without a leading space.
fn merge_tokens_into(tokens: &[RawToken], out: &mut Vec<WhisperWord>) {
    for t in tokens {
        if t.text.starts_with("[_") {
            continue;
        }
        let starts_new = t.text.starts_with(' ') || out.is_empty();
        let end = ms_to_s(t.offsets.to);
        if starts_new {
            out.push(WhisperWord {
                word: t.text.trim_start().to_string(),
                start: ms_to_s(t.offsets.from),
                end,
            });
        } else if let Some(last) = out.last_mut() {
            last.word.push_str(&t.text);
            last.end = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(text: &str, from: i64, to: i64) -> RawToken {
        RawToken {
            text: text.into(),
            offsets: Offsets { from, to },
        }
    }

    #[test]
    fn merges_subwords_and_punct() {
        let toks = vec![
            tok("[_BEG_]", 0, 0),
            tok(" Hello", 100, 500),
            tok(",", 500, 540),
            tok(" world", 600, 900),
            tok("ing", 900, 1100),
        ];
        let mut out = vec![];
        merge_tokens_into(&toks, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].word, "Hello,");
        assert!((out[0].start - 0.1).abs() < 1e-9);
        assert!((out[0].end - 0.54).abs() < 1e-9);
        assert_eq!(out[1].word, "worlding");
        assert!((out[1].end - 1.1).abs() < 1e-9);
    }
}
