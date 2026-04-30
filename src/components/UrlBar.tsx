import { useState, type FormEvent } from "react";

interface Props {
  isRunning: boolean;
  onSubmit: (url: string) => void;
  onCancel: () => void;
}

export function UrlBar({ isRunning, onSubmit, onCancel }: Props) {
  const [url, setUrl] = useState("");

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const trimmed = url.trim();
    if (!trimmed) return;
    onSubmit(trimmed);
  };

  return (
    <form className="url-bar" onSubmit={submit}>
      <input
        type="url"
        placeholder="Paste a YouTube URL…"
        value={url}
        onChange={(e) => setUrl(e.target.value)}
        spellCheck={false}
        autoFocus
      />
      {isRunning ? (
        <button type="button" className="cancel" onClick={onCancel}>
          Cancel
        </button>
      ) : (
        <button type="submit" disabled={!url.trim()}>
          Download
        </button>
      )}
    </form>
  );
}
