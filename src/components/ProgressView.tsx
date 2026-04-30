import type { ProgressEvent } from "../types";

const STAGE_LABEL: Record<ProgressEvent["stage"], string> = {
  download: "Downloading audio",
  convert: "Converting to WAV",
  transcribe: "Transcribing",
  harvest: "Harvesting clips",
  cache: "Loading cached transcript",
};

interface Props {
  progress: ProgressEvent | null;
}

export function ProgressView({ progress }: Props) {
  if (!progress) return null;
  const pct = Math.max(0, Math.min(1, progress.pct));
  return (
    <div className="progress">
      <div className="stage">{STAGE_LABEL[progress.stage] ?? progress.stage}</div>
      <div className="bar">
        <div className="fill" style={{ width: `${pct * 100}%` }} />
      </div>
      {progress.message && <div className="message">{progress.message}</div>}
    </div>
  );
}
