import { useEffect, useRef, useState } from "react";
import { UrlBar } from "./components/UrlBar";
import { ProgressView } from "./components/ProgressView";
import { SettingsPanel } from "./components/SettingsPanel";
import { ClipList } from "./components/ClipList";
import { PlayerControls } from "./components/PlayerControls";
import { Library } from "./components/Library";
import {
  onProgress,
  startTranscription,
  cancelTranscription,
  getDefaultOpenAIKey,
  listVideos,
  loadCached,
  deleteCached,
  exportBundle,
  convertFileSrc,
} from "./ipc";
import { useSettings } from "./hooks/useSettings";
import { useShadowPlayer } from "./hooks/useShadowPlayer";
import type {
  ProgressEvent,
  TranscribeResult,
  VideoSummary,
} from "./types";

export default function App() {
  const [settings, setSettings] = useSettings();
  const [isRunning, setIsRunning] = useState(false);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<TranscribeResult | null>(null);
  const [library, setLibrary] = useState<VideoSummary[]>([]);
  const unlistenRef = useRef<null | (() => void)>(null);

  const refreshLibrary = async () => {
    try {
      setLibrary(await listVideos());
    } catch {
      /* ignore */
    }
  };

  const audioRef = useRef<HTMLAudioElement | null>(null);
  const player = useShadowPlayer({
    audioRef: audioRef as React.RefObject<HTMLAudioElement>,
    clips: result?.clips ?? [],
    settings,
  });

  // Subscribe to progress events + load library on first mount.
  useEffect(() => {
    let cancelled = false;
    onProgress(setProgress).then((un) => {
      if (cancelled) un();
      else unlistenRef.current = un;
    });
    refreshLibrary();
    return () => {
      cancelled = true;
      unlistenRef.current?.();
    };
  }, []);

  // Prefill OpenAI key from dev .env on first launch.
  useEffect(() => {
    if (settings.openaiApiKey.trim()) return;
    let cancelled = false;
    getDefaultOpenAIKey()
      .then((key) => {
        if (cancelled || !key) return;
        setSettings({ ...settings, openaiApiKey: key, backend: "openai" });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
    // Run only once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Keyboard shortcuts: space=toggle, ←/→ prev/next, R=replay.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && /input|textarea|select/i.test(target.tagName)) return;
      if (!result || result.clips.length === 0) return;
      if (e.code === "Space") {
        e.preventDefault();
        player.toggle();
      } else if (e.code === "ArrowLeft") {
        e.preventDefault();
        player.prev();
      } else if (e.code === "ArrowRight") {
        e.preventDefault();
        player.next();
      } else if (e.key.toLowerCase() === "r") {
        e.preventDefault();
        player.replay();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [player, result]);

  async function run(url: string) {
    if (settings.backend === "openai" && !settings.openaiApiKey.trim()) {
      setError("Add your OpenAI API key in Settings to use the OpenAI backend.");
      return;
    }
    setError(null);
    setResult(null);
    setProgress(null);
    setIsRunning(true);
    try {
      const r = await startTranscription(url, {
        backend: settings.backend,
        openaiApiKey: settings.openaiApiKey,
      });
      setResult(r);
      refreshLibrary();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      if (msg !== "cancelled") setError(msg);
    } finally {
      setIsRunning(false);
    }
  }

  async function cancel() {
    try {
      await cancelTranscription();
    } catch {
      /* swallow */
    }
  }

  async function loadFromLibrary(videoId: string) {
    setError(null);
    try {
      const r = await loadCached(videoId);
      if (r) setResult(r);
      else {
        setError("That entry has gone stale. Re-paste the URL.");
        refreshLibrary();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  async function removeFromLibrary(videoId: string) {
    try {
      await deleteCached(videoId);
      if (result?.video_id === videoId) setResult(null);
      refreshLibrary();
    } catch {
      /* swallow */
    }
  }

  const audioSrc = result ? convertFileSrc(result.audio_path) : "";

  return (
    <div className="app">
      <h1>ShadowPlay</h1>
      <UrlBar isRunning={isRunning} onSubmit={run} onCancel={cancel} />
      <SettingsPanel
        settings={settings}
        onChange={setSettings}
        disabled={isRunning}
      />
      {isRunning && <ProgressView progress={progress} />}
      {error && <div className="error">{error}</div>}

      {/* Hidden audio sink. The timeupdate handler is attached via
          React's onTimeUpdate prop so it auto-binds to whatever element is
          mounted. No `key` needed — when audioSrc changes, React just
          updates the src attribute and the audio element reloads. */}
      {result && (
        <audio
          ref={audioRef}
          src={audioSrc}
          onTimeUpdate={player.onTimeUpdate}
          preload="auto"
          style={{ display: "none" }}
        />
      )}

      {result && result.clips.length > 0 && (
        <>
          <PlayerControls
            status={player.status}
            index={player.index}
            total={result.clips.length}
            repeatsLeft={player.repeatsLeft}
            totalRepeats={settings.repeats}
            onToggle={player.toggle}
            onPrev={player.prev}
            onNext={player.next}
            onReplay={player.replay}
          />
          <div className="export-row">
            <button
              type="button"
              className="export-btn"
              onClick={async () => {
                try {
                  const path = await exportBundle(result.video_id);
                  setError(null);
                  alert(
                    `Bundle ready for AirDrop:\n${path}\n\n` +
                      "Right-click in Finder → Share → AirDrop to your iPhone."
                  );
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e));
                }
              }}
            >
              Export for iPhone
            </button>
            <span className="export-hint">
              Saves a .shadowplay bundle to ~/Downloads.
            </span>
          </div>
        </>
      )}

      {result && result.clips.length === 0 && (
        <div className="success">
          Downloaded → <code>{result.audio_path}</code>
          <br />
          No clips harvested. (Filters dropped everything — try a longer or
          more conversational video.)
        </div>
      )}

      {result && (
        <ClipList
          clips={result.clips}
          currentIndex={player.index}
          onJump={player.jumpTo}
        />
      )}

      {!result && !isRunning && (
        <Library
          videos={library}
          onLoad={loadFromLibrary}
          onDelete={removeFromLibrary}
          onRefresh={refreshLibrary}
        />
      )}
    </div>
  );
}
