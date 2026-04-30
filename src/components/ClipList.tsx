import { useEffect, useRef } from "react";
import type { Clip } from "../types";

interface Props {
  clips: Clip[];
  currentIndex: number;
  onJump: (i: number) => void;
}

function formatTime(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function ClipList({ clips, currentIndex, onJump }: Props) {
  const activeRef = useRef<HTMLButtonElement | null>(null);

  // Keep the current row in view as the player advances.
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [currentIndex]);

  if (clips.length === 0) return null;

  return (
    <div className="clip-list">
      <div className="clip-list-header">
        {clips.length} clips · click to jump
      </div>
      <div className="clip-list-rows">
        {clips.map((c, i) => {
          const dur = ((c.end_ms - c.start_ms) / 1000).toFixed(1);
          const active = i === currentIndex;
          return (
            <button
              key={c.id}
              ref={active ? activeRef : null}
              type="button"
              className={`clip-row${active ? " active" : ""}`}
              onClick={() => onJump(i)}
            >
              <div className="clip-meta">
                <span className="clip-num">{i + 1}</span>
                <span className="clip-time">{formatTime(c.start_ms)}</span>
                <span className="clip-dur">{dur}s</span>
              </div>
              <div className="clip-text">{c.text}</div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
