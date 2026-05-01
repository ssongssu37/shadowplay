import type {
  AppSettings,
  Backend,
  ChunkingModel,
  GapMode,
  Repeats,
} from "../hooks/useSettings";

const MODEL_OPTIONS: {
  value: ChunkingModel;
  label: string;
  hint: string;
}[] = [
  { value: "gpt-4o-mini", label: "4o-mini", hint: "Cheapest (~$0.001/video)" },
  { value: "gpt-4.1-nano", label: "4.1-nano", hint: "Newer, similar cost" },
  { value: "gpt-4.1-mini", label: "4.1-mini", hint: "Better grammar (~$0.003)" },
  { value: "gpt-4.1", label: "4.1", hint: "High quality (~$0.02)" },
  { value: "gpt-4o", label: "4o", hint: "Premium fallback (~$0.02)" },
];

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

      {/* Smart chunking — usable whenever a key is set, even with the local
          backend. Falls back silently to punctuation-based harvesting if the
          API call fails. */}
      <div className="settings-row">
        <span className="settings-label">Smart chunking</span>
        <div className="segmented">
          <button
            type="button"
            className={settings.smartChunking ? "on" : ""}
            onClick={() => set({ smartChunking: true })}
            disabled={disabled || !settings.openaiApiKey.trim()}
            title={
              settings.openaiApiKey.trim()
                ? "Use gpt-4o-mini to split into thought groups"
                : "Requires an OpenAI API key"
            }
          >
            On
          </button>
          <button
            type="button"
            className={!settings.smartChunking ? "on" : ""}
            onClick={() => set({ smartChunking: false })}
            disabled={disabled}
          >
            Off
          </button>
        </div>
      </div>
      {settings.smartChunking && settings.openaiApiKey.trim() && (
        <>
          <div className="settings-row">
            <span className="settings-label">Chunker model</span>
            <div className="segmented">
              {MODEL_OPTIONS.map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  className={settings.chunkingModel === opt.value ? "on" : ""}
                  onClick={() => set({ chunkingModel: opt.value })}
                  disabled={disabled}
                  title={opt.hint}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>
          <div className="settings-row">
            <span className="settings-label">Min words</span>
            <div className="settings-slider">
              <input
                type="range"
                min={1}
                max={Math.max(1, settings.chunkMaxWords)}
                step={1}
                value={Math.min(settings.chunkMinWords, settings.chunkMaxWords)}
                onChange={(e) =>
                  set({ chunkMinWords: parseInt(e.target.value, 10) })
                }
                disabled={disabled}
              />
              <span className="settings-slider-value">
                ≥ {Math.min(settings.chunkMinWords, settings.chunkMaxWords)} words
              </span>
            </div>
          </div>
          <div className="settings-row">
            <span className="settings-label">Max words</span>
            <div className="settings-slider">
              <input
                type="range"
                min={4}
                max={25}
                step={1}
                value={settings.chunkMaxWords}
                onChange={(e) =>
                  set({ chunkMaxWords: parseInt(e.target.value, 10) })
                }
                disabled={disabled}
              />
              <span className="settings-slider-value">
                ≤ {settings.chunkMaxWords} words
              </span>
            </div>
          </div>
          <div className="settings-row">
            <span className="settings-label">Min length</span>
            <div className="settings-slider">
              <input
                type="range"
                min={0}
                max={settings.chunkMaxSeconds}
                step={0.5}
                value={Math.min(
                  settings.chunkMinSeconds,
                  settings.chunkMaxSeconds
                )}
                onChange={(e) =>
                  set({ chunkMinSeconds: parseFloat(e.target.value) })
                }
                disabled={disabled}
              />
              <span className="settings-slider-value">
                ≥ {Math.min(
                  settings.chunkMinSeconds,
                  settings.chunkMaxSeconds
                ).toFixed(1)}s
              </span>
            </div>
          </div>
          <div className="settings-row">
            <span className="settings-label">Max length</span>
            <div className="settings-slider">
              <input
                type="range"
                min={2}
                max={15}
                step={0.5}
                value={settings.chunkMaxSeconds}
                onChange={(e) =>
                  set({ chunkMaxSeconds: parseFloat(e.target.value) })
                }
                disabled={disabled}
              />
              <span className="settings-slider-value">
                ≤ {settings.chunkMaxSeconds.toFixed(1)}s
              </span>
            </div>
          </div>
        </>
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
