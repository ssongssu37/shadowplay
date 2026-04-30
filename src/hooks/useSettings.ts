import { useEffect, useState } from "react";

export type Backend = "local" | "openai";
export type GapMode = "auto" | "fixed";
export type Repeats = 1 | 2 | 3 | 4 | 5;

export interface AppSettings {
  backend: Backend;
  openaiApiKey: string;
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
  repeats: 2,
  gapMode: "auto",
  gapSeconds: 1.5,
  rate: 1.0,
};

function load(): AppSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULTS;
    const parsed = JSON.parse(raw) as Partial<AppSettings>;
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
