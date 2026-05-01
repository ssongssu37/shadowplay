import { useEffect, useState } from "react";

export type Backend = "local" | "openai";
export type GapMode = "auto" | "fixed";
export type Repeats = 1 | 2 | 3 | 4 | 5;
export type ChunkingModel =
  | "gpt-4o-mini"
  | "gpt-4o"
  | "gpt-4.1-mini"
  | "gpt-4.1"
  | "gpt-4.1-nano";

export interface AppSettings {
  backend: Backend;
  openaiApiKey: string;
  // Smart chunking — when an OpenAI key is set, send the raw transcript to
  // gpt-4o-mini for thought-group segmentation instead of relying on
  // Whisper's punctuation. ~$0.0005 per video.
  smartChunking: boolean;
  chunkingModel: ChunkingModel; // OpenAI model id used by the two-pass chunker
  chunkMaxWords: number; // 4..25, hard upper bound on words per thought group
  chunkMaxSeconds: number; // 2..15, hard upper bound on clip duration
  chunkMinWords: number; // 1..max, floor — short clips get merged
  chunkMinSeconds: number; // 0..max, floor — short clips get merged
  // Player settings
  repeats: Repeats;
  gapMode: GapMode;
  gapSeconds: number; // used when gapMode === "fixed", range 0.5..10
  rate: number; // 0.5..1.5
}

const STORAGE_KEY = "shadowplay.settings";

const DEFAULTS: AppSettings = {
  backend: "local",
  openaiApiKey: "",
  smartChunking: true,
  chunkingModel: "gpt-4o-mini",
  chunkMaxWords: 12,
  chunkMaxSeconds: 5,
  chunkMinWords: 4,
  chunkMinSeconds: 1.0,
  repeats: 2,
  gapMode: "auto",
  gapSeconds: 1.5,
  rate: 1.0,
};

function load(): AppSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULTS;
    const parsed = JSON.parse(raw) as Partial<AppSettings> & {
      chunkTargetWords?: number;
    };
    // Migrate the old name introduced briefly during development.
    if (
      parsed.chunkTargetWords !== undefined &&
      parsed.chunkMaxWords === undefined
    ) {
      parsed.chunkMaxWords = parsed.chunkTargetWords;
    }
    return { ...DEFAULTS, ...parsed };
  } catch {
    return DEFAULTS;
  }
}

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>(load);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    } catch {
      /* quota / private mode — ignore */
    }
  }, [settings]);

  return [settings, setSettings] as const;
}
