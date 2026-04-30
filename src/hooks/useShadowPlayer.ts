import {
  useCallback,
  useEffect,
  useReducer,
  useRef,
  type RefObject,
} from "react";
import type { Clip } from "../types";
import type { AppSettings } from "./useSettings";

export type PlayerStatus =
  | "idle"     // no clips loaded
  | "playing"  // audio rolling within current clip
  | "paused"   // user paused, or between actions
  | "gap"      // waiting between clips (timer counting down)
  | "ended";   // walked off the end of the queue

interface State {
  status: PlayerStatus;
  index: number;
  repeatsLeft: number; // additional plays remaining for the current clip
}

type Action =
  | { type: "LOAD" }
  | { type: "PLAY"; repeatsLeft: number }
  | { type: "PAUSE" }
  | { type: "JUMP"; index: number; repeatsLeft: number }
  | { type: "REPLAY"; repeatsLeft: number }
  | { type: "REPEAT_DECREMENT" }
  | { type: "ENTER_GAP" }
  | { type: "ADVANCE"; repeatsLeft: number }
  | { type: "ENDED" };

interface Args {
  audioRef: RefObject<HTMLAudioElement>;
  clips: Clip[];
  settings: Pick<AppSettings, "repeats" | "gapMode" | "gapSeconds" | "rate">;
}

interface Return {
  status: PlayerStatus;
  index: number;
  repeatsLeft: number; // additional plays REMAINING for current clip
  play: () => void;
  pause: () => void;
  toggle: () => void;
  next: () => void;
  prev: () => void;
  jumpTo: (i: number) => void;
  replay: () => void;
  /** Attach to <audio onTimeUpdate={...}>. Drives the player loop. */
  onTimeUpdate: () => void;
}

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "LOAD":
      return { status: "paused", index: 0, repeatsLeft: 0 };
    case "PLAY":
      return { ...state, status: "playing", repeatsLeft: action.repeatsLeft };
    case "PAUSE":
      return { ...state, status: "paused" };
    case "REPEAT_DECREMENT":
      return {
        ...state,
        status: "playing",
        repeatsLeft: Math.max(0, state.repeatsLeft - 1),
      };
    case "ENTER_GAP":
      return { ...state, status: "gap" };
    case "ADVANCE":
      return {
        ...state,
        status: "playing",
        index: state.index + 1,
        repeatsLeft: action.repeatsLeft,
      };
    case "ENDED":
      return { ...state, status: "ended" };
    case "JUMP":
      return {
        ...state,
        status: "playing",
        index: action.index,
        repeatsLeft: action.repeatsLeft,
      };
    case "REPLAY":
      return { ...state, status: "playing", repeatsLeft: action.repeatsLeft };
    default:
      return state;
  }
}

export function useShadowPlayer({ audioRef, clips, settings }: Args): Return {
  const [state, rawDispatch] = useReducer(reducer, {
    status: "idle",
    index: 0,
    repeatsLeft: 0,
  });

  // Refs let our event handlers always see the latest values without
  // re-binding listeners on every state change.
  const stateRef = useRef(state);
  stateRef.current = state;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const clipsRef = useRef(clips);
  clipsRef.current = clips;
  const gapTimerRef = useRef<number | null>(null);

  // Update stateRef SYNCHRONOUSLY before queuing the React render. Without
  // this, audio events that fire in the ~16ms window between dispatch and
  // render would read stale state — e.g. after ADVANCE the timeupdate
  // handler would still think we're on the previous clip and trigger a
  // bogus end-of-clip transition, skipping a line.
  const dispatch = useCallback((action: Action) => {
    stateRef.current = reducer(stateRef.current, action);
    rawDispatch(action);
  }, []);

  // Number of additional plays for the next clip the player starts.
  const initialRepeats = () => Math.max(0, settingsRef.current.repeats - 1);

  // ---- core helpers ------------------------------------------------------

  const clearGap = useCallback(() => {
    if (gapTimerRef.current != null) {
      clearTimeout(gapTimerRef.current);
      gapTimerRef.current = null;
    }
  }, []);

  const seekToCurrent = useCallback(() => {
    const audio = audioRef.current;
    const clip = clipsRef.current[stateRef.current.index];
    if (!audio || !clip) return;
    audio.currentTime = clip.start_ms / 1000;
  }, [audioRef]);

  const playFromStart = useCallback(() => {
    const audio = audioRef.current;
    const clip = clipsRef.current[stateRef.current.index];
    if (!audio || !clip) return;
    audio.playbackRate = settingsRef.current.rate;
    audio.currentTime = clip.start_ms / 1000;
    void audio.play();
  }, [audioRef]);

  // ---- on clips load -----------------------------------------------------

  useEffect(() => {
    clearGap();
    if (clips.length === 0) {
      dispatch({ type: "PAUSE" });
      return;
    }
    dispatch({ type: "LOAD" });
    // Seek the audio to the first clip's start so the user can hit play
    // immediately without the audio first playing the file's lead-in.
    const audio = audioRef.current;
    if (audio) {
      audio.pause();
      audio.currentTime = clips[0].start_ms / 1000;
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clips]);

  // ---- timeupdate driver -------------------------------------------------
  //
  // Attached via React's onTimeUpdate prop on the <audio> element so the
  // listener binds correctly every time the element (re)mounts — no
  // imperative addEventListener with timing-sensitive ref availability.

  const onTimeUpdate = useCallback(() => {
    const audio = audioRef.current;
    if (!audio) return;
    const s = stateRef.current;
    if (s.status !== "playing") return;
    const clip = clipsRef.current[s.index];
    if (!clip) return;
    const endSec = clip.end_ms / 1000;
    if (audio.currentTime < endSec) return;

    // Reached the end of the current clip. ALWAYS enter the gap — both
    // repeats of the same clip and transitions to the next clip get a gap
    // (so the user always has time to shadow aloud before the next play).
    audio.pause();

    const settings = settingsRef.current;
    const gapMs =
      settings.gapMode === "auto"
        ? Math.max(150, clip.end_ms - clip.start_ms)
        : Math.max(0, settings.gapSeconds * 1000);

    dispatch({ type: "ENTER_GAP" });

    gapTimerRef.current = window.setTimeout(() => {
      gapTimerRef.current = null;
      const cur = stateRef.current;

      // Repeats of the current clip use the gap too — replay after waiting.
      if (cur.repeatsLeft > 0) {
        dispatch({ type: "REPEAT_DECREMENT" });
        audio.currentTime = clip.start_ms / 1000;
        void audio.play();
        return;
      }

      const nextIdx = cur.index + 1;
      if (nextIdx >= clipsRef.current.length) {
        dispatch({ type: "ENDED" });
        return;
      }
      const nextClip = clipsRef.current[nextIdx];
      dispatch({ type: "ADVANCE", repeatsLeft: initialRepeats() });
      audio.playbackRate = settingsRef.current.rate;
      audio.currentTime = nextClip.start_ms / 1000;
      void audio.play();
    }, gapMs);
  }, [audioRef]);

  // Keep audio playbackRate in sync with the settings while playing.
  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;
    audio.playbackRate = settings.rate;
  }, [audioRef, settings.rate]);

  // ---- public commands ---------------------------------------------------

  const play = useCallback(() => {
    if (clipsRef.current.length === 0) return;
    clearGap();
    dispatch({ type: "PLAY", repeatsLeft: initialRepeats() });
    playFromStart();
  // initialRepeats reads from a ref; safe to omit from deps.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clearGap, playFromStart]);

  const pause = useCallback(() => {
    clearGap();
    audioRef.current?.pause();
    dispatch({ type: "PAUSE" });
  }, [audioRef, clearGap]);

  const toggle = useCallback(() => {
    const s = stateRef.current.status;
    if (s === "playing" || s === "gap") pause();
    else play();
  }, [play, pause]);

  const jumpTo = useCallback(
    (i: number) => {
      const len = clipsRef.current.length;
      if (i < 0 || i >= len) return;
      clearGap();
      const audio = audioRef.current;
      const clip = clipsRef.current[i];
      audio?.pause();
      dispatch({ type: "JUMP", index: i, repeatsLeft: initialRepeats() });
      // Seek + play imperatively so we don't depend on the reducer's index
      // having flushed (playFromStart reads stateRef.current.index).
      if (audio && clip) {
        audio.playbackRate = settingsRef.current.rate;
        audio.currentTime = clip.start_ms / 1000;
        void audio.play();
      }
    // initialRepeats reads from a ref; safe to omit from deps.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    },
    [audioRef, clearGap]
  );

  const next = useCallback(() => {
    const cur = stateRef.current.index;
    if (cur + 1 < clipsRef.current.length) jumpTo(cur + 1);
  }, [jumpTo]);

  const prev = useCallback(() => {
    const cur = stateRef.current.index;
    if (cur > 0) jumpTo(cur - 1);
  }, [jumpTo]);

  const replay = useCallback(() => {
    clearGap();
    audioRef.current?.pause();
    dispatch({ type: "REPLAY", repeatsLeft: initialRepeats() });
    seekToCurrent();
    void audioRef.current?.play();
  // initialRepeats reads from a ref; safe to omit from deps.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [audioRef, clearGap, seekToCurrent]);

  // ---- cleanup -----------------------------------------------------------

  useEffect(() => () => clearGap(), [clearGap]);

  return {
    status: state.status,
    index: state.index,
    repeatsLeft: state.repeatsLeft,
    play,
    pause,
    toggle,
    next,
    prev,
    jumpTo,
    replay,
    onTimeUpdate,
  };
}
