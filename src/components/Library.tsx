import type { VideoSummary } from "../types";

interface Props {
  videos: VideoSummary[];
  onLoad: (videoId: string) => void;
  onDelete: (videoId: string) => void;
  onRefresh: () => void;
}

function relativeTime(unixSec: number): string {
  const now = Date.now() / 1000;
  const diff = Math.max(0, now - unixSec);
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)}d ago`;
  return new Date(unixSec * 1000).toLocaleDateString();
}

export function Library({ videos, onLoad, onDelete, onRefresh }: Props) {
  return (
    <div className="library">
      <div className="library-header">
        <span>Library · {videos.length}</span>
        <button
          type="button"
          className="library-refresh"
          onClick={onRefresh}
          title="Refresh"
          aria-label="Refresh library"
        >
          ↻
        </button>
      </div>
      {videos.length === 0 && (
        <div className="library-empty">
          No saved videos yet. Paste a YouTube URL above to start.
        </div>
      )}
      <div className="library-rows">
        {videos.map((v) => (
          <div key={v.video_id} className="library-row">
            <button
              type="button"
              className="library-load"
              onClick={() => onLoad(v.video_id)}
              title="Load this video"
            >
              <div className="library-title">
                {v.title || v.video_id}
              </div>
              <div className="library-meta">
                <span>{v.clip_count} clips</span>
                <span>·</span>
                <span>{relativeTime(v.fetched_at)}</span>
              </div>
            </button>
            <button
              type="button"
              className="library-delete"
              onClick={(e) => {
                e.stopPropagation();
                if (
                  confirm(
                    `Delete "${v.title || v.video_id}" and its audio file?`
                  )
                ) {
                  onDelete(v.video_id);
                }
              }}
              title="Delete from library"
              aria-label="Delete"
            >
              ✕
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
