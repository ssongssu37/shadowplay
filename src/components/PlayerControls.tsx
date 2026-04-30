import type { PlayerStatus } from "../hooks/useShadowPlayer";

interface Props {
  status: PlayerStatus;
  index: number;
  total: number;
  repeatsLeft: number;
  totalRepeats: number;
  onToggle: () => void;
  onPrev: () => void;
  onNext: () => void;
  onReplay: () => void;
  disabled?: boolean;
}

export function PlayerControls({
  status,
  index,
  total,
  repeatsLeft,
  totalRepeats,
  onToggle,
  onPrev,
  onNext,
  onReplay,
  disabled,
}: Props) {
  const isPlaying = status === "playing" || status === "gap";
  // currentPlay = how many times we've played so far (1..totalRepeats).
  const currentPlay = totalRepeats - repeatsLeft;
  return (
    <div className="player">
      <button
        type="button"
        className="player-btn"
        onClick={onPrev}
        disabled={disabled || index === 0}
        title="Previous clip"
        aria-label="Previous clip"
      >
        ◀◀
      </button>
      <button
        type="button"
        className="player-btn primary"
        onClick={onToggle}
        disabled={disabled}
        title={isPlaying ? "Pause" : "Play"}
        aria-label={isPlaying ? "Pause" : "Play"}
      >
        {isPlaying ? "❚❚" : "▶"}
      </button>
      <button
        type="button"
        className="player-btn"
        onClick={onReplay}
        disabled={disabled}
        title="Replay current clip"
        aria-label="Replay"
      >
        ↺
      </button>
      <button
        type="button"
        className="player-btn"
        onClick={onNext}
        disabled={disabled || index + 1 >= total}
        title="Next clip"
        aria-label="Next clip"
      >
        ▶▶
      </button>
      <div className="player-meta">
        <span className="player-clip">
          Clip <strong>{Math.min(index + 1, total || 1)}</strong> of {total}
        </span>
        {totalRepeats > 1 && (
          <span className="player-repeat" title="Plays of current clip">
            {Math.max(1, Math.min(currentPlay, totalRepeats))}/{totalRepeats}
          </span>
        )}
        {status === "gap" && <span className="player-state">gap…</span>}
        {status === "ended" && <span className="player-state">end</span>}
      </div>
    </div>
  );
}
