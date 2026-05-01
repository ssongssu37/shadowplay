//! Semantic chunking via gpt-4o-mini.
//!
//! Two-pass design:
//!   PASS A: ask the LLM for SENTENCE/CLAUSE boundaries with no length cap.
//!           This gives us authoritative natural boundaries.
//!   PASS B: for any sentence that exceeds the user's max_words/max_seconds,
//!           ask the LLM a focused follow-up to subdivide that sentence into
//!           shadowing-sized chunks at natural mid-clause breaks.
//!
//! The clips come back annotated with `sentence_end_ms` — millisecond
//! timestamps where pass A said a sentence ended. Downstream merging uses
//! that to forbid cross-sentence joins (so "...thank you for HubSpot."
//! never fuses with "For sponsoring this video...").
//!
//! After chunking, `enforce_clip_bounds` runs three deterministic passes:
//!   1. enforce_max  — split anything over the user's caps
//!   2. enforce_min  — merge anything under the user's floors,
//!                     respecting sentence boundaries
//!   3. boundary_fix — if a clip ends on a stop word ("a", "the", "for",
//!                     "your", "pretty"...), extend its tail by 1–3 words
//!                     into the next clip's territory (overlap allowed)
//!                     until it lands on a content word.
//!
//! Cost: pass A is one call per ~1500-word window; pass B is one call per
//! oversized sentence. For a typical 5-minute video that's 1–3 calls,
//! ~$0.001 total on gpt-4o-mini.

use crate::commands::transcription::Clip;
use crate::error::{AppError, AppResult};
use crate::pipeline::whisper::WhisperWord;
use serde::Deserialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Whitelist the user-pickable models. Anything outside this set falls back
/// to DEFAULT_MODEL so a typo'd settings value can't break the app.
const ALLOWED_MODELS: &[&str] = &[
    "gpt-4o-mini",
    "gpt-4o",
    "gpt-4.1-mini",
    "gpt-4.1",
    "gpt-4.1-nano",
];

fn pick_model(requested: &str) -> &'static str {
    for &m in ALLOWED_MODELS {
        if m == requested {
            return m;
        }
    }
    DEFAULT_MODEL
}

/// Words per pass-A LLM call. Long videos are split into windows of this size.
const WINDOW_WORDS: usize = 1500;

/// Output of the LLM chunker. `sentence_end_ms` is consumed by the merge
/// pass to forbid cross-sentence joins.
pub struct ChunkResult {
    pub clips: Vec<Clip>,
    pub sentence_end_ms: Vec<u32>,
}

/// Run the two-pass LLM chunker.
pub async fn chunk_with_llm(
    words: &[WhisperWord],
    api_key: &str,
    max_words: u32,
    max_seconds: f64,
    model: &str,
    cancel: CancellationToken,
) -> AppResult<ChunkResult> {
    let model = pick_model(model);
    if api_key.trim().is_empty() {
        return Err(AppError::Other("OpenAI API key is empty".into()));
    }
    if words.is_empty() {
        return Ok(ChunkResult { clips: vec![], sentence_end_ms: vec![] });
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;

    // ── PASS A: sentence boundaries (no length cap) ──────────────────────
    let mut sentence_ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    while start < words.len() {
        let end = (start + WINDOW_WORDS).min(words.len());
        let window = &words[start..end];
        let local = pass_a_sentences(
            window,
            api_key,
            model,
            &client,
            cancel.clone(),
            start > 0,
            end < words.len(),
        )
        .await?;
        for (s, e) in local {
            sentence_ranges.push((start + s, start + e));
        }
        start = end;
    }
    let sentence_ranges = validate_and_repair(sentence_ranges, words.len())?;

    // ── PASS B: subdivide oversized sentences ────────────────────────────
    let max_w = max_words as usize;
    let max_s = max_seconds;
    let mut final_chunks: Vec<(usize, usize, bool)> = Vec::new();
    for (s, e) in &sentence_ranges {
        let span_w = e - s + 1;
        let span_s = words[*e].end - words[*s].start;

        if span_w <= max_w && span_s <= max_s {
            final_chunks.push((*s, *e, true));
            continue;
        }
        // Oversized. Try pass B.
        let sub_window = &words[*s..=*e];
        let sub_result = pass_b_subdivide(
            sub_window,
            api_key,
            max_words,
            model,
            &client,
            cancel.clone(),
        )
        .await;
        match sub_result {
            Ok(local_ranges) if !local_ranges.is_empty() => {
                let validated = validate_and_repair(local_ranges, sub_window.len())
                    .unwrap_or_else(|_| vec![(0, sub_window.len() - 1)]);
                let last = validated.len().saturating_sub(1);
                for (i, (sub_s, sub_e)) in validated.into_iter().enumerate() {
                    final_chunks.push((s + sub_s, s + sub_e, i == last));
                }
            }
            _ => {
                // Pass B failed — emit as-is; enforce_max will bisect.
                final_chunks.push((*s, *e, true));
            }
        }
    }

    // Build clips + extract sentence-end timestamps.
    let clips: Vec<Clip> = final_chunks
        .iter()
        .filter_map(|(s, e, _)| build_one_clip(*s, *e, words))
        .collect();
    let sentence_end_ms: Vec<u32> = final_chunks
        .iter()
        .filter(|(_, _, is_end)| *is_end)
        .filter_map(|(_, e, _)| {
            words.get(*e).map(|w| (w.end * 1000.0).round().max(0.0) as u32)
        })
        .collect();

    Ok(ChunkResult { clips, sentence_end_ms })
}

// ──────────────────────────────────────────────────────────────────────────
// PASS A — sentence boundaries
// ──────────────────────────────────────────────────────────────────────────

async fn pass_a_sentences(
    window: &[WhisperWord],
    api_key: &str,
    model: &str,
    client: &reqwest::Client,
    cancel: CancellationToken,
    is_continuation: bool,
    has_more: bool,
) -> AppResult<Vec<(usize, usize)>> {
    let prompt = prompt_pass_a(window, is_continuation, has_more);
    call_chat_for_groups(
        client,
        api_key,
        model,
        cancel,
        "You are a sentence detector for an English shadowing app. \
         You always return valid JSON with a `groups` field.",
        &prompt,
    )
    .await
}

fn prompt_pass_a(words: &[WhisperWord], is_continuation: bool, has_more: bool) -> String {
    let last_idx = words.len().saturating_sub(1);
    let mut s = String::new();
    s.push_str(
        "Split this English transcript into COMPLETE SENTENCES. \
The transcript has had punctuation removed; you must judge purely from the words \
where one sentence ends and the next begins.\n\n\
A sentence is a complete thought. Examples of where to break:\n\
- Subject change (\"...so we did that. I just got back from Coachella.\")\n\
- Topic shift (\"...is so powerful. Today we are gonna talk about...\")\n\
- After a complete clause when a new clause begins with a clear sentence-starter \
  (I, We, You, He, She, They, It, So, And then, Now, Today, Yesterday).\n\n\
DO NOT split mid-clause. Keep these together inside one sentence:\n\
- Restrictive clauses (\"...the video that he made\")\n\
- Phrases like \"even though\", \"as well as\", \"in order to\", \"a lot of\"\n\
- Subordinate clauses (\"...because\", \"...when\", \"...if\")\n\
- Predicate continuations: if you see \"...why it's\", do not stop at \"why\" — \
  \"it's\" is the start of the predicate that completes the thought.\n\n\
HARD RULE: every sentence must contain at least 6 words. NEVER emit a 1-word \
or 2-word sentence. If a clause feels short, fold it into the preceding or \
following sentence.\n\n\
Sentences may be LONG (40+ words) — there is no upper cap on sentence length. \
A subdivider runs after you to break long sentences into shadowing-sized pieces.\n\n",
    );
    s.push_str("Rules:\n");
    s.push_str(
        "- Cover every word in order. Sentences are contiguous and non-overlapping.\n\
- Don't end a sentence on a preposition, article, conjunction, auxiliary verb, \
  or possessive pronoun unless it's clearly a sentence-ending fragment.\n",
    );
    if is_continuation {
        s.push_str(
            "- This window may BEGIN mid-sentence. Start your first range at 0 anyway.\n",
        );
    }
    if has_more {
        s.push_str(
            "- This window may END mid-sentence. Your last range must end at the final index.\n",
        );
    }
    s.push_str(&format!(
        "- The first range must start at 0; the last must end at index {last_idx}.\n\n\
Return ONLY JSON in this shape:\n\
{{\"groups\":[[start_idx, end_idx], ...]}}\n\
Indices are inclusive.\n\n\
Words (no punctuation):\n"
    ));
    for (i, w) in words.iter().enumerate() {
        let cleaned = clean_word_for_prompt(&w.word);
        if cleaned.is_empty() {
            s.push_str(&format!("[{i}] _\n"));
        } else {
            s.push_str(&format!("[{i}] {cleaned}\n"));
        }
    }
    s
}

// ──────────────────────────────────────────────────────────────────────────
// PASS B — subdivide one long sentence
// ──────────────────────────────────────────────────────────────────────────

async fn pass_b_subdivide(
    sentence_words: &[WhisperWord],
    api_key: &str,
    max_words: u32,
    model: &str,
    client: &reqwest::Client,
    cancel: CancellationToken,
) -> AppResult<Vec<(usize, usize)>> {
    let prompt = prompt_pass_b(sentence_words, max_words);
    call_chat_for_groups(
        client,
        api_key,
        model,
        cancel,
        "You split English sentences into shadowing-friendly chunks for language learners. \
         You always return valid JSON with a `groups` field.",
        &prompt,
    )
    .await
}

fn prompt_pass_b(words: &[WhisperWord], max_words: u32) -> String {
    let last_idx = words.len().saturating_sub(1);
    let min_words = 4u32;
    let mut s = String::new();
    s.push_str(
        "This is ONE English sentence (or one long clause), unpunctuated. \
Split it into shadowing chunks for an ESL learner to repeat one at a time.\n\n",
    );
    s.push_str(&format!(
        "HARD LIMIT: each chunk MUST be between {min_words} and {max_words} words. \
NEVER exceed {max_words}.\n\n",
    ));
    s.push_str(
        "Rules for where to break:\n\
- End each chunk at a natural sub-clause boundary, not mid-phrase.\n\
- Clause-starters that should BEGIN the next chunk, not end the current one:\n\
    how, why, where, when, what, who, which, that, if, because, since, although, \
    while, before, after, until, unless, and, but, so, or.\n\
- Never split fixed phrases: \"even though\", \"as well as\", \"in order to\", \
  \"a lot of\", \"kind of\", \"sort of\", \"because of\".\n\
- Don't end a chunk on these stop words: a, an, the, of, to, for, in, on, at, \
  with, by, from, is, was, are, were, am, be, been, being, have, has, had, \
  do, does, did, will, would, can, could, should, may, might, and, but, or, \
  so, that, which, who, whose, your, my, his, her, their, its, our, very, \
  really, pretty, just.\n\
- Cover every word in order. Chunks are contiguous and non-overlapping.\n",
    );
    s.push_str(&format!(
        "- The first chunk must start at 0; the last must end at index {last_idx}.\n\n\
Return ONLY JSON:\n\
{{\"groups\":[[start_idx, end_idx], ...]}}\n\
Indices are inclusive.\n\n\
Words:\n"
    ));
    for (i, w) in words.iter().enumerate() {
        let cleaned = clean_word_for_prompt(&w.word);
        if cleaned.is_empty() {
            s.push_str(&format!("[{i}] _\n"));
        } else {
            s.push_str(&format!("[{i}] {cleaned}\n"));
        }
    }
    s
}

// ──────────────────────────────────────────────────────────────────────────
// Shared LLM call wrapper
// ──────────────────────────────────────────────────────────────────────────

async fn call_chat_for_groups(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    cancel: CancellationToken,
    system_prompt: &str,
    user_prompt: &str,
) -> AppResult<Vec<(usize, usize)>> {
    let body = serde_json::json!({
        "model": model,
        "response_format": { "type": "json_object" },
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ]
    });

    let req = client
        .post(ENDPOINT)
        .bearer_auth(api_key)
        .json(&body)
        .send();

    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
        r = req => r.map_err(|e| AppError::Other(format!("openai network: {e}")))?,
    };

    let status = resp.status();
    let bytes = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
        b = resp.bytes() => b.map_err(|e| AppError::Other(e.to_string()))?,
    };
    if !status.is_success() {
        let snippet = String::from_utf8_lossy(&bytes);
        return Err(AppError::Other(format!(
            "openai chat http {}: {}",
            status,
            snippet.chars().take(400).collect::<String>()
        )));
    }

    let parsed: ChatCompletion = serde_json::from_slice(&bytes)?;
    let content = parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();
    if content.trim().is_empty() {
        return Err(AppError::Other("openai returned empty content".into()));
    }
    parse_groups(&content)
}

fn clean_word_for_prompt(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '"' | '“' | '”'))
        .to_string()
}

// ──────────────────────────────────────────────────────────────────────────
// Public bounds-enforcement pipeline
// ──────────────────────────────────────────────────────────────────────────

/// Final post-processing applied after both the LLM and harvester paths.
///
///   1. enforce_max  — split anything over caps
///   2. enforce_min  — merge anything under floors (sentence-aware: forbids
///                     joins across `sentence_end_ms` boundaries)
///   3. boundary_fix — extend any clip ending on a stop word into the next
///                     clip's territory (overlap allowed) until it lands on
///                     a content word.
///
/// `sentence_end_ms` may be empty (harvester path with no info), in which
/// case merges are not sentence-restricted.
/// `words` may be empty (no precise timestamp data), in which case
/// boundary_fix is skipped.
pub fn enforce_clip_bounds(
    clips: Vec<Clip>,
    sentence_end_ms: &[u32],
    words: &[WhisperWord],
    max_words: u32,
    max_seconds: f64,
    min_words: u32,
    min_seconds: f64,
) -> Vec<Clip> {
    let split = enforce_max(clips, max_words, max_seconds);
    let merged = enforce_min(
        split,
        sentence_end_ms,
        max_words,
        max_seconds,
        min_words,
        min_seconds,
    );
    if words.is_empty() {
        merged
    } else {
        boundary_fix(merged, words, max_words, max_seconds)
    }
}

// ──────────────────────────────────────────────────────────────────────────
// enforce_max
// ──────────────────────────────────────────────────────────────────────────

fn enforce_max(clips: Vec<Clip>, max_words: u32, max_seconds: f64) -> Vec<Clip> {
    let max_w = (max_words as usize).max(4);
    let max_s = max_seconds.max(1.5);
    let mut out = Vec::with_capacity(clips.len());
    for clip in clips {
        let dur = (clip.end_ms as f64 - clip.start_ms as f64) / 1000.0;
        let wc = clip.text.split_whitespace().count();
        if wc <= max_w && dur <= max_s {
            out.push(clip);
            continue;
        }
        let pieces = split_text_into_pieces(&clip.text, max_w);
        let total_chars: usize = pieces.iter().map(|p| p.chars().count()).sum::<usize>().max(1);
        let mut cursor_ms = clip.start_ms as f64;
        let total_ms = clip.end_ms as f64 - clip.start_ms as f64;
        for piece in pieces {
            let frac = piece.chars().count() as f64 / total_chars as f64;
            let piece_ms = total_ms * frac;
            let next_ms = cursor_ms + piece_ms;
            out.push(Clip {
                id: Uuid::new_v4().to_string(),
                start_ms: cursor_ms.round() as u32,
                end_ms: next_ms.round() as u32,
                text: piece,
            });
            cursor_ms = next_ms;
        }
    }
    out
}

fn split_text_into_pieces(text: &str, max_words: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        return vec![text.to_string()];
    }
    words
        .chunks(max_words)
        .map(|c| {
            let s = c.join(" ");
            if s.ends_with(['.', '!', '?']) {
                s
            } else {
                format!("{s}.")
            }
        })
        .collect()
}

// ──────────────────────────────────────────────────────────────────────────
// enforce_min — sentence-aware merging
// ──────────────────────────────────────────────────────────────────────────

const SENTENCE_BOUNDARY_TOL_MS: i64 = 200;

fn is_sentence_end(end_ms: u32, sentence_end_ms: &[u32]) -> bool {
    sentence_end_ms
        .iter()
        .any(|&t| (t as i64 - end_ms as i64).abs() <= SENTENCE_BOUNDARY_TOL_MS)
}

fn enforce_min(
    clips: Vec<Clip>,
    sentence_end_ms: &[u32],
    max_words: u32,
    max_seconds: f64,
    min_words: u32,
    min_seconds: f64,
) -> Vec<Clip> {
    if clips.is_empty() {
        return clips;
    }
    let max_w = (max_words as usize).max(4);
    let max_s = max_seconds.max(1.5);
    let min_w = (min_words as usize).clamp(1, max_w);
    let min_s = min_seconds.clamp(0.0, max_s);

    fn count(c: &Clip) -> (usize, f64) {
        (
            c.text.split_whitespace().count(),
            (c.end_ms as f64 - c.start_ms as f64) / 1000.0,
        )
    }
    fn fits(merged_w: usize, merged_s: f64, max_w: usize, max_s: f64) -> bool {
        merged_w <= max_w && merged_s <= max_s
    }
    fn join(a: &Clip, b: &Clip) -> Clip {
        let mut text = a
            .text
            .trim_end_matches(|c: char| matches!(c, '.' | '!' | '?'))
            .to_string();
        text.push(' ');
        text.push_str(b.text.trim_start());
        Clip {
            id: Uuid::new_v4().to_string(),
            start_ms: a.start_ms.min(b.start_ms),
            end_ms: a.end_ms.max(b.end_ms),
            text,
        }
    }

    // Two-tier merge policy when a clip is below floor:
    //   1. Try same-sentence forward (preferred — keep meaning intact)
    //   2. Try same-sentence backward
    //   3. Cross-sentence forward (acceptable since the user explicitly
    //      asked for a higher floor than Pass A produced — many "sentence
    //      ends" from Pass A are actually fragments)
    //   4. Cross-sentence backward
    //
    // Repeat the merge until the result is at floor OR no merge fits.
    // This fixes the [shortA][shortB][shortC] case where ABC fits but a
    // single-pass would only produce AB+C.

    let mut out: Vec<Clip> = Vec::with_capacity(clips.len());
    let mut iter = clips.into_iter().peekable();
    while let Some(mut cur) = iter.next() {
        loop {
            let (wc, dur) = count(&cur);
            if wc >= min_w && dur >= min_s {
                break;
            }

            let cur_is_sentence_end = is_sentence_end(cur.end_ms, sentence_end_ms);

            // Try same-sentence forward.
            if !cur_is_sentence_end {
                if let Some(next) = iter.peek() {
                    let (nwc, ndur) = count(next);
                    if fits(wc + nwc, dur + ndur, max_w, max_s) {
                        let next = iter.next().unwrap();
                        cur = join(&cur, &next);
                        continue;
                    }
                }
            }

            // Try same-sentence backward.
            if let Some(prev) = out.last() {
                let prev_is_sentence_end = is_sentence_end(prev.end_ms, sentence_end_ms);
                if !prev_is_sentence_end {
                    let (pwc, pdur) = count(prev);
                    if fits(pwc + wc, pdur + dur, max_w, max_s) {
                        let prev = out.pop().unwrap();
                        cur = join(&prev, &cur);
                        continue;
                    }
                }
            }

            // Cross-sentence forward.
            if let Some(next) = iter.peek() {
                let (nwc, ndur) = count(next);
                if fits(wc + nwc, dur + ndur, max_w, max_s) {
                    let next = iter.next().unwrap();
                    cur = join(&cur, &next);
                    continue;
                }
            }

            // Cross-sentence backward.
            if let Some(prev) = out.last() {
                let (pwc, pdur) = count(prev);
                if fits(pwc + wc, pdur + dur, max_w, max_s) {
                    let prev = out.pop().unwrap();
                    cur = join(&prev, &cur);
                    continue;
                }
            }

            // LAST RESORT: a tiny fragment with no neighbor that fits within
            // strict caps. Floor violations are worse than slight ceiling
            // violations — better a 5.5s clip than a 0.3s one. Allow up to
            // +3 words / +1.5s over cap, picking the cheaper direction.
            let relaxed_w = max_w + 3;
            let relaxed_s = max_s + 1.5;
            let fwd_cost = iter.peek().map(|n| {
                let (nwc, ndur) = count(n);
                let mw = wc + nwc;
                let ms = dur + ndur;
                if mw <= relaxed_w && ms <= relaxed_s {
                    Some((mw as i64 - max_w as i64).max(0) + ((ms - max_s).max(0.0) * 10.0) as i64)
                } else {
                    None
                }
            }).flatten();
            let bwd_cost = out.last().map(|p| {
                let (pwc, pdur) = count(p);
                let mw = pwc + wc;
                let ms = pdur + dur;
                if mw <= relaxed_w && ms <= relaxed_s {
                    Some((mw as i64 - max_w as i64).max(0) + ((ms - max_s).max(0.0) * 10.0) as i64)
                } else {
                    None
                }
            }).flatten();
            match (fwd_cost, bwd_cost) {
                (Some(fc), Some(bc)) if fc <= bc => {
                    let next = iter.next().unwrap();
                    cur = join(&cur, &next);
                    continue;
                }
                (Some(_), None) => {
                    let next = iter.next().unwrap();
                    cur = join(&cur, &next);
                    continue;
                }
                (_, Some(_)) => {
                    let prev = out.pop().unwrap();
                    cur = join(&prev, &cur);
                    continue;
                }
                _ => {}
            }

            // Nothing fits even relaxed. Leave it.
            break;
        }
        out.push(cur);
    }
    out
}

// ──────────────────────────────────────────────────────────────────────────
// boundary_fix — extend tails ending on stop words
// ──────────────────────────────────────────────────────────────────────────

/// Stop words that almost never end a real thought group. If a clip ends
/// here, it's almost certainly mid-phrase and worth extending into the
/// next clip's territory.
const FORBIDDEN_TAIL_WORDS: &[&str] = &[
    // Articles
    "a", "an", "the",
    // Prepositions
    "of", "to", "for", "in", "on", "at", "with", "by", "from", "into",
    "onto", "about", "as", "than",
    // Auxiliaries / be / have / do
    "is", "was", "are", "were", "am", "be", "been", "being",
    "have", "has", "had", "having",
    "do", "does", "did", "doing",
    "will", "would", "can", "could", "should", "may", "might", "must", "shall",
    // Conjunctions / subordinators
    "and", "but", "or", "so", "yet", "nor", "if", "because", "while", "since",
    "though", "although", "unless", "before", "after", "until",
    // Determiners / possessives / demonstratives
    "that", "which", "who", "whom", "whose",
    "your", "my", "his", "her", "their", "its", "our",
    "this", "these", "those",
    // Intensifiers / fillers
    "very", "really", "pretty", "just", "kinda", "sorta", "like", "even", "still",
];

/// For each clip whose final word is a stop word, extend forward by up to
/// 3 words (pulled from the global word stream) until it lands on a content
/// word. The next clip is left intact — overlap is allowed.
///
/// Tolerance: extension may exceed the user's max_words and max_seconds by
/// a small grace amount (+3 words, +1.5s) so the fix doesn't get blocked
/// for clips that are already near the cap.
fn boundary_fix(
    clips: Vec<Clip>,
    words: &[WhisperWord],
    max_words: u32,
    max_seconds: f64,
) -> Vec<Clip> {
    let max_w = max_words as usize + 3;
    let max_s = max_seconds + 1.5;

    let mut out = Vec::with_capacity(clips.len());
    for clip in clips {
        let Some((first_idx, mut last_idx)) = find_word_range(&clip, words) else {
            out.push(clip);
            continue;
        };

        let mut extensions = 0;
        while extensions < 3 && last_idx + 1 < words.len() {
            let cur_last = strip_for_check(&words[last_idx].word);
            if !FORBIDDEN_TAIL_WORDS.contains(&cur_last.as_str()) {
                break;
            }
            let new_last = last_idx + 1;
            let new_word_count = new_last - first_idx + 1;
            let new_dur = words[new_last].end - words[first_idx].start;
            if new_word_count > max_w || new_dur > max_s {
                break;
            }
            last_idx = new_last;
            extensions += 1;
        }

        if extensions == 0 {
            out.push(clip);
            continue;
        }

        // Rebuild text + end_ms from the extended span.
        let span = &words[first_idx..=last_idx];
        let raw = span
            .iter()
            .map(|w| w.word.trim())
            .collect::<Vec<_>>()
            .join(" ")
            .replace(" ,", ",")
            .replace(" .", ".")
            .replace(" !", "!")
            .replace(" ?", "?")
            .replace("  ", " ");
        let text = capitalize_first_and_terminate(raw.trim());
        out.push(Clip {
            id: clip.id,
            start_ms: clip.start_ms, // unchanged
            end_ms: (span.last().unwrap().end * 1000.0).round().max(0.0) as u32,
            text,
        });
    }
    out
}

fn find_word_range(clip: &Clip, words: &[WhisperWord]) -> Option<(usize, usize)> {
    let s = clip.start_ms as f64 / 1000.0;
    let e = clip.end_ms as f64 / 1000.0;
    let first = words
        .iter()
        .position(|w| w.start >= s - 0.05 || w.end > s)?;
    let last = words.iter().rposition(|w| w.end <= e + 0.05)?;
    if last < first {
        return None;
    }
    Some((first, last))
}

fn strip_for_check(s: &str) -> String {
    s.trim()
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
        .to_lowercase()
}

// ──────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ChatCompletion {
    choices: Vec<ChatChoice>,
}
#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}
#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct GroupsPayload {
    groups: Vec<Vec<usize>>,
}

fn parse_groups(content: &str) -> AppResult<Vec<(usize, usize)>> {
    let json_str = extract_json_object(content).unwrap_or_else(|| content.to_string());
    let payload: GroupsPayload = serde_json::from_str(&json_str)
        .map_err(|e| AppError::Other(format!("llm json: {e}; raw: {json_str}")))?;
    let mut out = Vec::with_capacity(payload.groups.len());
    for g in payload.groups {
        if g.len() != 2 {
            return Err(AppError::Other(format!(
                "expected [start, end] pairs, got {g:?}"
            )));
        }
        out.push((g[0], g[1]));
    }
    Ok(out)
}

fn extract_json_object(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut end = start;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    Some(s[start..=end].to_string())
}

fn validate_and_repair(
    groups: Vec<(usize, usize)>,
    total: usize,
) -> AppResult<Vec<(usize, usize)>> {
    if total == 0 {
        return Ok(vec![]);
    }
    if groups.is_empty() {
        return Err(AppError::Other("llm returned no groups".into()));
    }

    let mut repaired: Vec<(usize, usize)> = Vec::with_capacity(groups.len());
    for (s, e) in groups {
        if s > e || e >= total {
            return Err(AppError::Other(format!(
                "out-of-bounds or inverted range [{s},{e}] (total={total})"
            )));
        }
        if let Some(last) = repaired.last_mut() {
            if s <= last.1 {
                if s == last.1 + 1 {
                    repaired.push((s, e));
                } else if s > last.0 && e >= last.1 {
                    last.1 = e;
                } else {
                    return Err(AppError::Other(format!(
                        "overlapping ranges: prev=({},{}) cur=({s},{e})",
                        last.0, last.1
                    )));
                }
            } else if s > last.1 + 1 {
                last.1 = e;
            } else {
                repaired.push((s, e));
            }
        } else {
            repaired.push((0, e.max(s)));
        }
    }

    if let Some(last) = repaired.last_mut() {
        if last.1 < total - 1 {
            last.1 = total - 1;
        }
    }
    Ok(repaired)
}

fn build_one_clip(s: usize, e: usize, words: &[WhisperWord]) -> Option<Clip> {
    if s >= words.len() || e >= words.len() || e < s {
        return None;
    }
    let span = &words[s..=e];
    let text = span
        .iter()
        .map(|w| w.word.trim())
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace("  ", " ");
    let text = capitalize_first_and_terminate(text.trim());
    let start = span.first().unwrap().start;
    let end = span.last().unwrap().end;
    if end <= start {
        return None;
    }
    Some(Clip {
        id: Uuid::new_v4().to_string(),
        start_ms: (start * 1000.0).round().max(0.0) as u32,
        end_ms: (end * 1000.0).round().max(0.0) as u32,
        text,
    })
}

fn capitalize_first_and_terminate(s: &str) -> String {
    let mut chars = s.chars();
    let mut out = match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => return String::new(),
    };
    if let Some(last) = out.chars().last() {
        if !matches!(last, '.' | '!' | '?') {
            while let Some(c) = out.chars().last() {
                if matches!(c, ',' | ';' | ':' | '-' | '—') {
                    out.pop();
                    out = out.trim().to_string();
                } else {
                    break;
                }
            }
            out.push('.');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ww(word: &str, start: f64, end: f64) -> WhisperWord {
        WhisperWord {
            word: word.into(),
            start,
            end,
        }
    }

    #[test]
    fn extracts_json_from_fenced_content() {
        let s = "```json\n{\"groups\":[[0,2],[3,5]]}\n```";
        let extracted = extract_json_object(s).unwrap();
        assert!(extracted.contains("groups"));
    }

    #[test]
    fn validate_fills_trailing_gap() {
        let r = validate_and_repair(vec![(0, 3), (4, 7)], 10).unwrap();
        assert_eq!(r.last().unwrap().1, 9);
    }

    #[test]
    fn validate_swallows_interior_gap() {
        let r = validate_and_repair(vec![(0, 3), (6, 9)], 10).unwrap();
        assert_eq!(r, vec![(0, 9)]);
    }

    #[test]
    fn validate_rejects_overlap() {
        let err = validate_and_repair(vec![(0, 5), (3, 7)], 10);
        assert!(err.is_err());
    }

    #[test]
    fn boundary_fix_extends_into_next_clip() {
        // Words: "I love a big red apple" 0..6, ending on "a" should extend.
        let words: Vec<WhisperWord> = vec![
            ww("I", 0.0, 0.2),
            ww("love", 0.2, 0.5),
            ww("a", 0.5, 0.6),
            ww("big", 0.6, 0.9),
            ww("red", 0.9, 1.2),
            ww("apple", 1.2, 1.6),
        ];
        let clips = vec![
            Clip {
                id: "1".into(),
                start_ms: 0,
                end_ms: 600,
                text: "I love a.".into(),
            },
            Clip {
                id: "2".into(),
                start_ms: 600,
                end_ms: 1600,
                text: "Big red apple.".into(),
            },
        ];
        let fixed = boundary_fix(clips, &words, 10, 5.0);
        // First clip should now end on "big" (or further), not "a".
        assert!(
            !fixed[0].text.to_lowercase().trim_end_matches('.').ends_with(" a"),
            "first clip should not end on 'a': {}",
            fixed[0].text
        );
    }

    #[test]
    fn enforce_min_prefers_same_sentence_merge() {
        // Two short clips inside the same sentence should merge forward
        // (no sentence boundary between them).
        let clips = vec![
            Clip {
                id: "1".into(),
                start_ms: 0,
                end_ms: 800,
                text: "I went.".into(),
            },
            Clip {
                id: "2".into(),
                start_ms: 800,
                end_ms: 1600,
                text: "To the store.".into(),
            },
        ];
        let sentence_ends: Vec<u32> = vec![1600];
        let result = enforce_min(clips, &sentence_ends, 12, 5.0, 4, 1.0);
        assert_eq!(result.len(), 1, "should merge to 1 clip: {:?}", result);
    }

    #[test]
    fn enforce_min_falls_back_to_cross_sentence_when_floor_demands() {
        // Three clips, all short. Sentence boundary between every pair
        // — strict same-sentence rule would leave them all short.
        // Two-tier policy should still merge to honor floor.
        let clips = vec![
            Clip {
                id: "1".into(),
                start_ms: 0,
                end_ms: 800,
                text: "Hi there.".into(),
            },
            Clip {
                id: "2".into(),
                start_ms: 800,
                end_ms: 1600,
                text: "Bye now.".into(),
            },
            Clip {
                id: "3".into(),
                start_ms: 1600,
                end_ms: 2400,
                text: "See ya.".into(),
            },
        ];
        let sentence_ends = vec![800u32, 1600u32, 2400u32];
        let result = enforce_min(clips, &sentence_ends, 12, 5.0, 4, 1.0);
        // Cross-sentence merge allowed because floor isn't satisfied.
        assert!(
            result.len() < 3,
            "should merge across sentence boundaries when forced: {:?}",
            result
        );
    }
}
