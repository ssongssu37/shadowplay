import type {
  AppSettings,
  Backend,
  GapMode,
  Repeats,
} from "../hooks/useSettings";

interface Props {
  settings: AppSettings;
  onChange: (s: AppSettings) => void;
  disabled?: boolean; // disable transcription-related fields while a job runs
}

export function SettingsPanel({ settings, onChange, disabled }: Props) {
  const set = (patch: Partial<AppSettings>) =>
    onChange({ ...settings, ...patch });

  const setBackend = (backend: Backend) => set({ backend });
  const setRepeats = (repeats: Repeats) => set({ repeats });
  const setGapMode = (gapMode: GapMode) => set({ gapMode });

  return (
    <div className="settings">
      {/* Transcription backend */}
      <div className="settings-row">
        <span className="settings-label">Transcription</span>
        <div className="segmented">
          <button
            type="button"
            className={settings.backend === "local" ? "on" : ""}
            onClick={() => setBackend("local")}
            disabled={disabled}
          >
            Local
          </button>
          <button
            type="button"
            className={settings.backend === "openai" ? "on" : ""}
            onClick={() => setBackend("openai")}
            disabled={disabled}
          >
            OpenAI
          </button>
        </div>
      </div>
      {settings.backend === "openai" && (
        <div className="settings-row">
          <span className="settings-label">API key</span>
          <input
            type="password"
            placeholder="sk-…"
            value={settings.openaiApiKey}
            onChange={(e) => set({ openaiApiKey: e.target.value })}
            spellCheck={false}
            autoComplete="off"
            disabled={disabled}
          />
        </div>
      )}

      {/* Player settings — always editable, even mid-playback */}
      <div className="settings-divider" />
      <div className="settings-row">
        <span className="settings-label">Repeat each</span>
        <div className="segmented">
          {([1, 2, 3, 4, 5] as Repeats[]).map((n) => (
            <button
              key={n}
              type="button"
              className={settings.repeats === n ? "on" : ""}
              onClick={() => setRepeats(n)}
            >
              {n}×
            </button>
          ))}
        </div>
      </div>

      <div className="settings-row">
        <span className="settings-label">Gap</span>
        <div className="segmented">
          <button
            type="button"
            className={settings.gapMode === "auto" ? "on" : ""}
            onClick={() => setGapMode("auto")}
            title="Gap = clip's own duration (good for shadowing aloud)"
          >
            Auto
          </button>
          <button
            type="button"
            className={settings.gapMode === "fixed" ? "on" : ""}
            onClick={() => setGapMode("fixed")}
          >
            Fixed
          </button>
        </div>
        {settings.gapMode === "fixed" && (
          <div className="settings-slider">
            <input
              type="range"
              min={0.5}
              max={10}
              step={0.5}
              value={settings.gapSeconds}
              onChange={(e) =>
                set({ gapSeconds: parseFloat(e.target.value) })
              }
            />
            <span className="settings-slider-value">
              {settings.gapSeconds.toFixed(1)}s
            </span>
          </div>
        )}
      </div>

      <div className="settings-row">
        <span className="settings-label">Speed</span>
        <div className="segmented">
          {[0.75, 1.0, 1.25].map((r) => (
            <button
              key={r}
              type="button"
              className={Math.abs(settings.rate - r) < 0.01 ? "on" : ""}
              onClick={() => set({ rate: r })}
            >
              {r === 1 ? "1×" : `${r}×`}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-hint">
        {settings.backend === "local"
          ? "Local whisper.cpp — free, offline. Slow on Intel Macs."
          : "OpenAI Whisper API — fast (~10s/5min), $0.006/min, key stays local."}
      </div>
    </div>
  );
}
