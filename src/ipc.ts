import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ProgressEvent, TranscribeResult, VideoSummary } from "./types";

export const onProgress = (
  cb: (e: ProgressEvent) => void
): Promise<UnlistenFn> =>
  listen<ProgressEvent>("transcription-progress", (e) => cb(e.payload));

export interface TranscribeOptions {
  backend: "local" | "openai";
  openaiApiKey?: string;
}

export const startTranscription = (
  url: string,
  options: TranscribeOptions
) =>
  invoke<TranscribeResult>("start_transcription", {
    url,
    options: {
      backend: options.backend,
      openaiApiKey: options.openaiApiKey ?? "",
    },
  });

export const cancelTranscription = () =>
  invoke<void>("cancel_transcription");

export const getDefaultOpenAIKey = () =>
  invoke<string>("get_default_openai_key");

export const listVideos = () => invoke<VideoSummary[]>("list_videos");

export const loadCached = (videoId: string) =>
  invoke<TranscribeResult | null>("load_cached", { videoId });

export const deleteCached = (videoId: string) =>
  invoke<void>("delete_cached", { videoId });

export const exportBundle = (videoId: string) =>
  invoke<string>("export_bundle", { videoId });

export { convertFileSrc };
