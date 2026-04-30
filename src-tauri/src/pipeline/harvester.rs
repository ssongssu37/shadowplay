//! Sentence harvester — port of `SentenceHarvester.harvestFromSegments` from
//! `/Users/mac/Desktop/ClipTalk-Mac/ClipTalk/Services/SentenceHarvester.swift`,
//! retuned for shadowing-friendly clip lengths.
//!
//! Pipeline:
//! 1. Group consecutive whisper segments into "raw sentences" — split each
//!    time a segment ends with `.!?`.
//! 2. Within each group, also split on interior terminators (whisper often
//!    packs multiple sentences into one segment), proportionally divide the
//!    time range, and snap to actual word boundaries (±0.05s).
//! 3. Any candidate exceeding MAX_WORDS or MAX_DURATION_S gets greedy-split
//!    on clause breaks (comma/dash) or before leading conjunctions.
//! 4. Filter to [MIN_DURATION_S, MAX_DURATION_S] and [MIN_WORDS, MAX_WORDS].

use crate::commands::transcription::Clip;
use crate::pipeline::whisper::{WhisperSegment, WhisperWord};
use uuid::Uuid;

pub const MAX_WORDS: usize = 20;
pub const MIN_WORDS: usize = 3;
pub const MAX_DURATION_S: f64 = 5.0;
pub const MIN_DURATION_S: f64 = 1.0;
pub const SNAP_TOLERANCE_S: f64 = 0.05;

/// Words that indicate a clean clause boundary when they START the next
/// chunk. We split BEFORE these and drop them from the new chunk.
const LEADING_CONJUNCTIONS: &[&str] = &[
    "and", "but", "so", "or", "because", "however", "yet", "though",
    "while", "if", "when", "as", "since", "although", "unless",
];

#[derive(Debug, Clone)]
struct Candidate {
    text: String,
    start: f64,
    end: f64,
}

impl Candidate {
    fn duration(&self) -> f64 {
        self.end - self.start
    }
    fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

pub fn harvest(segments: &[WhisperSegment], words: &[WhisperWord]) -> Vec<Clip> {
    if segments.is_empty() {
        return vec![];
    }

    // Step 1: group consecutive segments into raw sentences. Split each time
    // a segment's text ends in . ! or ?.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        current.push(i);
        let trimmed = seg.text.trim();
        if let Some(last) = trimmed.chars().last() {
            if matches!(last, '.' | '!' | '?') {
                groups.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }

    // Step 2: split each group on interior terminators, time-slice
    // proportionally, snap to word boundaries.
    let mut candidates: Vec<Candidate> = Vec::new();
    for group in &groups {
        let Some(&first_idx) = group.first() else { continue };
        let Some(&last_idx) = group.last() else { continue };
        let group_start = segments[first_idx].start;
        let group_end = segments[last_idx].end;

        let joined = group
            .iter()
            .map(|&i| segments[i].text.trim())
            .collect::<Vec<_>>()
            .join(" ")
            .replace("  ", " ")
            .trim()
            .to_string();
        if joined.is_empty() {
            continue;
        }

        let pieces = split_into_sentences(&joined);
        let total_chars = joined.chars().count().max(1);
        let mut char_cursor = 0usize;
        for piece in pieces {
            let piece_len = piece.chars().count();
            let trimmed = piece.trim();
            if trimmed.is_empty() {
                char_cursor += piece_len;
                continue;
            }
            let start_frac = char_cursor as f64 / total_chars as f64;
            char_cursor += piece_len;
            let end_frac = char_cursor as f64 / total_chars as f64;
            let approx_start = group_start + (group_end - group_start) * start_frac;
            let approx_end = group_start + (group_end - group_start) * end_frac;
            let (s, e) = snap_to_words(approx_start, approx_end, words);
            if e <= s {
                continue;
            }
            candidates.push(Candidate {
                text: normalize_sentence(trimmed),
                start: s,
                end: e,
            });
        }
    }

    // Step 3: split oversized candidates.
    let mut expanded: Vec<Candidate> = Vec::new();
    for cand in candidates {
        if cand.word_count() <= MAX_WORDS && cand.duration() <= MAX_DURATION_S {
            expanded.push(cand);
            continue;
        }
        expanded.extend(split_to_fit(&cand, words));
    }

    // Step 4: filter by length bounds, convert to public Clip type.
    expanded
        .into_iter()
        .filter(|c| {
            c.duration() >= MIN_DURATION_S
                && c.duration() <= MAX_DURATION_S
                && c.word_count() >= MIN_WORDS
                && c.word_count() <= MAX_WORDS
        })
        .map(|c| Clip {
            id: Uuid::new_v4().to_string(),
            start_ms: (c.start * 1000.0).round().max(0.0) as u32,
            end_ms: (c.end * 1000.0).round().max(0.0) as u32,
            text: c.text,
        })
        .collect()
}

/// Split text on sentence terminators (.!?) followed by whitespace or EOS.
/// Keeps the punctuation with the preceding piece.
fn split_into_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        current.push(chars[i]);
        let is_terminator = matches!(chars[i], '.' | '!' | '?');
        let next_is_boundary = i + 1 >= chars.len() || chars[i + 1].is_whitespace();
        if is_terminator && next_is_boundary {
            out.push(std::mem::take(&mut current));
            // Skip whitespace after terminator.
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// Snap (start, end) to actual whisper-word boundaries within the range
/// (±SNAP_TOLERANCE_S). Strips silence padding baked into segments.
fn snap_to_words(start: f64, end: f64, words: &[WhisperWord]) -> (f64, f64) {
    if words.is_empty() || end <= start {
        return (start, end);
    }
    let first = words
        .iter()
        .find(|w| w.start >= start - SNAP_TOLERANCE_S && w.start < end);
    let last = words
        .iter()
        .rev()
        .find(|w| w.end > start && w.end <= end + SNAP_TOLERANCE_S);
    let s = first.map(|w| w.start).unwrap_or(start);
    let e = last.map(|w| w.end).unwrap_or(end);
    (s, e.max(s + 0.1))
}

/// Strip trailing junk and append a period if missing.
fn normalize_sentence(s: &str) -> String {
    let mut t = s.to_string();
    loop {
        match t.chars().last() {
            Some(c) if matches!(c, ',' | ';' | ':' | '-' | '—') => {
                t.pop();
                t = t.trim().to_string();
            }
            _ => break,
        }
    }
    if let Some(last) = t.chars().last() {
        if !matches!(last, '.' | '!' | '?') {
            t.push('.');
        }
    }
    t
}

fn strip_punct(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Greedy word walk: largest contiguous span that fits, preferring cuts at
/// `,;-—` or just before a leading conjunction.
fn split_to_fit(cand: &Candidate, all_words: &[WhisperWord]) -> Vec<Candidate> {
    let inside: Vec<&WhisperWord> = all_words
        .iter()
        .filter(|w| {
            w.start >= cand.start - SNAP_TOLERANCE_S
                && w.end <= cand.end + SNAP_TOLERANCE_S
        })
        .collect();
    if inside.len() < MIN_WORDS {
        return vec![];
    }

    let mut out: Vec<Candidate> = Vec::new();
    let mut cursor = 0usize;
    while cursor < inside.len() {
        // Skip leading conjunctions.
        while cursor < inside.len() {
            let stripped = strip_punct(&inside[cursor].word).to_lowercase();
            if LEADING_CONJUNCTIONS.contains(&stripped.as_str()) {
                cursor += 1;
            } else {
                break;
            }
        }
        if cursor >= inside.len() {
            break;
        }

        let chunk_start = cursor;
        let mut chunk_end = cursor;
        let mut best_break: Option<usize> = None;
        for i in cursor..inside.len() {
            let dur = inside[i].end - inside[chunk_start].start;
            let wc = i - chunk_start + 1;
            if wc > MAX_WORDS || dur > MAX_DURATION_S {
                break;
            }
            chunk_end = i;

            let trimmed = inside[i].word.trim();
            let ends_break = trimmed
                .chars()
                .last()
                .is_some_and(|c| matches!(c, ',' | ';' | '-' | '—'));
            let next_is_conj = if i + 1 < inside.len() {
                let s = strip_punct(&inside[i + 1].word).to_lowercase();
                LEADING_CONJUNCTIONS.contains(&s.as_str())
            } else {
                false
            };
            if ends_break || next_is_conj {
                best_break = Some(i);
            }
        }

        let cut_at = best_break.unwrap_or(chunk_end);
        let span = &inside[chunk_start..=cut_at];
        let wc = span.len();
        if wc >= MIN_WORDS {
            let raw = span
                .iter()
                .map(|w| w.word.trim())
                .collect::<Vec<_>>()
                .join(" ")
                .replace(" ,", ",")
                .replace(" .", ".")
                .replace(" !", "!")
                .replace(" ?", "?")
                .replace("  ", " ")
                .trim()
                .to_string();
            let text = capitalize_first(&normalize_sentence(&raw));
            let s = span.first().unwrap().start;
            let e = span.last().unwrap().end;
            if e > s {
                out.push(Candidate { text, start: s, end: e });
            }
        }
        cursor = cut_at + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, start: f64, end: f64) -> WhisperSegment {
        WhisperSegment {
            text: text.into(),
            start,
            end,
        }
    }

    fn word(w: &str, start: f64, end: f64) -> WhisperWord {
        WhisperWord {
            word: w.into(),
            start,
            end,
        }
    }

    #[test]
    fn splits_on_terminal_punctuation() {
        let segs = vec![
            seg("Hello there friend.", 0.0, 2.0),
            seg("How are you doing today?", 2.0, 4.0),
        ];
        // Word stream — one word per token, evenly spaced for simplicity.
        let words: Vec<WhisperWord> = "Hello there friend. How are you doing today?"
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| word(w, i as f64 * 0.4, (i as f64 + 1.0) * 0.4))
            .collect();
        let clips = harvest(&segs, &words);
        assert_eq!(clips.len(), 2, "expected 2 clips, got {:?}", clips);
        assert!(clips[0].text.starts_with("Hello"));
        assert!(clips[1].text.starts_with("How"));
    }

    #[test]
    fn drops_too_short_clips() {
        // Single 2-word segment should be dropped (under MIN_WORDS=3).
        let segs = vec![seg("Yeah.", 0.0, 0.5)];
        let words = vec![word("Yeah.", 0.0, 0.5)];
        assert!(harvest(&segs, &words).is_empty());
    }

    #[test]
    fn caps_long_clips_at_max_words() {
        // Construct a 30-word sentence — should be split into 2 chunks.
        let mut text = String::new();
        let mut words = Vec::new();
        for i in 0..30 {
            if i > 0 {
                text.push(' ');
            }
            text.push_str(&format!("word{i}"));
            words.push(word(&format!("word{i}"), i as f64 * 0.2, (i as f64 + 1.0) * 0.2));
        }
        text.push('.');
        let segs = vec![seg(&text, 0.0, 6.0)];
        let clips = harvest(&segs, &words);
        // Since there are no clause-break punctuations, the greedy split caps
        // at MAX_WORDS or MAX_DURATION_S whichever hits first.
        assert!(!clips.is_empty(), "expected at least one clip");
        for c in &clips {
            let wc = c.text.split_whitespace().count();
            assert!(wc <= MAX_WORDS, "clip exceeds MAX_WORDS: {} ({})", wc, c.text);
            let dur = (c.end_ms as f64 - c.start_ms as f64) / 1000.0;
            assert!(
                dur <= MAX_DURATION_S + 0.01,
                "clip exceeds MAX_DURATION_S: {}",
                dur
            );
        }
    }
}
