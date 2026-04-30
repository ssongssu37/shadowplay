// Frontend mirrors of Rust types. Keep in sync with src-tauri/src/.

export interface Clip {
  id: string;
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface TranscribeResult {
  video_id: string;
  title: string | null;
  audio_path: string;
  clips: Clip[];
  from_cache: boolean;
}

export type Stage =
  | "download"
  | "convert"
  | "transcribe"
  | "harvest"
  | "cache";

export interface ProgressEvent {
  stage: Stage;
  pct: number; // 0..1
  message?: string;
}

export interface VideoSummary {
  video_id: string;
  title: string | null;
  clip_count: number;
  fetched_at: number;
  audio_path: string;
}
