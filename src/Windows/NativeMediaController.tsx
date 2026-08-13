import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import {
  getNativePlayerState,
  getDesktopVideoHostWindow,
  listenNativePlayerEvents,
  localPathToFileUri,
  readNativePlayerStartupStatus,
  nativePlayerPause,
  nativePlayerPlay,
  nativePlayerSeek,
  nativePlayerSelectAudioTrack,
  nativePlayerSelectSubtitleTrack,
  nativePlayerSetLooping,
  nativePlayerSetMuted,
  nativePlayerSetRate,
  nativePlayerSetSubtitleUri,
  nativePlayerSetVolume,
  type NativePlayerState,
} from "../Services/gstreamerPlayer";
import "./NativeMediaController.css";

const EMPTY_STATE: NativePlayerState = {
  generation: 0,
  status: "stopped",
  positionMs: null,
  durationMs: null,
  volume: 1,
  muted: false,
  rate: 1,
  looping: false,
  bufferingPercent: null,
  title: null,
  audioTracks: [],
  subtitleTracks: [],
  externalSubtitleUri: null,
};

const RATES = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];

type PlayerGlyphName = "back" | "close" | "forward" | "fullscreen" | "more" | "pause" | "play" | "repeat" | "volume" | "volumeOff";

function PlayerGlyph({ name }: { name: PlayerGlyphName }) {
  const paths: Record<PlayerGlyphName, ReactNode> = {
    back: <><path d="M8.5 7.5 4.8 11l3.7 3.5"/><path d="M5.2 11h6.3a4.5 4.5 0 1 1 0 9"/></>,
    close: <><path d="m7 7 10 10"/><path d="M17 7 7 17"/></>,
    forward: <><path d="m15.5 7.5 3.7 3.5-3.7 3.5"/><path d="M18.8 11h-6.3a4.5 4.5 0 1 0 0 9"/></>,
    fullscreen: <><path d="M8.5 4.5h-4v4"/><path d="M15.5 4.5h4v4"/><path d="M8.5 19.5h-4v-4"/><path d="M15.5 19.5h4v-4"/></>,
    more: <><circle cx="6" cy="12" r="1" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1" fill="currentColor" stroke="none"/><circle cx="18" cy="12" r="1" fill="currentColor" stroke="none"/></>,
    pause: <><path d="M9 7v10"/><path d="M15 7v10"/></>,
    play: <path d="m9 6.8 8.2 5.2L9 17.2Z" fill="currentColor" stroke="none"/>,
    repeat: <><path d="m17 7 2.5 2.5L17 12"/><path d="M4.5 10A4 4 0 0 1 8.5 6h10.3"/><path d="m7 17-2.5-2.5L7 12"/><path d="M19.5 14a4 4 0 0 1-4 4H5.2"/></>,
    volume: <><path d="M5 10v4h3l4 3V7l-4 3Z"/><path d="M15 9.5a3.5 3.5 0 0 1 0 5"/><path d="M17.5 7a7 7 0 0 1 0 10"/></>,
    volumeOff: <><path d="M5 10v4h3l4 3V7l-4 3Z"/><path d="m16 10 4 4"/><path d="m20 10-4 4"/></>,
  };
  return <svg className="native-player-glyph" viewBox="0 0 24 24" aria-hidden="true">{paths[name]}</svg>;
}

function formatTime(milliseconds: number | null): string {
  if (milliseconds == null || !Number.isFinite(milliseconds)) return "--:--";
  const totalSeconds = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export default function NativeMediaController() {
  const { t } = useTranslation();
  const [state, setState] = useState<NativePlayerState>(EMPTY_STATE);
  const [seekDraft, setSeekDraft] = useState<number | null>(null);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [error, setError] = useState("");
  const [startupMessage, setStartupMessage] = useState("");
  const hideTimer = useRef<number | null>(null);
  const stateRef = useRef(state);
  stateRef.current = state;
  // getCurrentWindow() constructs a new JavaScript handle. Keep one stable instance so polling
  // renders do not repeatedly tear down and re-register move, resize, and startup listeners.
  const playerWindow = useMemo(() => getCurrentWindow(), []);

  const refresh = useCallback(async () => {
    try {
      setState(await getNativePlayerState());
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  const run = useCallback(async (operation: () => Promise<void>) => {
    try {
      await operation();
      setError("");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    }
  }, [refresh]);

  const scheduleControlsHide = useCallback((delay: number) => {
    if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => {
      if (stateRef.current.status === "playing") setControlsVisible(false);
    }, delay);
  }, []);

  const revealControls = useCallback(() => {
    setControlsVisible(true);
    scheduleControlsHide(1_350);
  }, [scheduleControlsHide]);

  const hideControlsSoon = useCallback(() => {
    scheduleControlsHide(420);
  }, [scheduleControlsHide]);

  const keepControlsVisible = useCallback(() => {
    if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
    setControlsVisible(true);
  }, []);

  const seekRelative = useCallback((differenceMs: number) => {
    const current = stateRef.current;
    const target = Math.max(0, Math.min(
      (current.positionMs || 0) + differenceMs,
      current.durationMs || Number.MAX_SAFE_INTEGER,
    ));
    void run(() => nativePlayerSeek(target));
  }, [run]);

  useEffect(() => {
    let active = true;
    const poll = async () => {
      if (active) await refresh();
    };
    void poll();
    const timer = window.setInterval(poll, 400);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [refresh]);

  useEffect(() => {
    const refreshStartup = () => {
      const startup = readNativePlayerStartupStatus();
      if (!startup) return;
      if (startup.phase === "error") {
        setError(startup.message || t("Playback failed."));
        setStartupMessage("");
      } else if (startup.phase === "preparing") {
        setStartupMessage(t("Preparing cloud stream..."));
      } else if (startup.phase === "opening") {
        setStartupMessage(t("Opening native decoder..."));
      } else {
        setStartupMessage("");
      }
    };
    refreshStartup();
    const timer = window.setInterval(refreshStartup, 200);
    return () => window.clearInterval(timer);
  }, [t]);

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    let synchronizing = false;
    let synchronizeAgain = false;
    const synchronizeVideoHost = () => {
      if (synchronizing) {
        synchronizeAgain = true;
        return;
      }
      synchronizing = true;
      void (async () => {
        do {
          synchronizeAgain = false;
          const videoHost = await getDesktopVideoHostWindow();
          if (!videoHost || disposed) return;
          const [position, size] = await Promise.all([
            playerWindow.outerPosition(),
            playerWindow.innerSize(),
          ]);
          if (disposed) return;
          await videoHost.setPosition(position);
          await videoHost.setSize(size);
        } while (synchronizeAgain && !disposed);
      })().catch((reason) => setError(String(reason))).finally(() => {
        synchronizing = false;
        if (synchronizeAgain && !disposed) synchronizeVideoHost();
      });
    };

    void Promise.all([
      playerWindow.onMoved(synchronizeVideoHost),
      playerWindow.onResized(synchronizeVideoHost),
    ]).then((dispose) => {
      if (disposed) dispose.forEach((unlisten) => unlisten());
      else unlisteners.push(...dispose);
    }).catch((reason) => setError(String(reason)));
    synchronizeVideoHost();

    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [playerWindow]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenNativePlayerEvents((event) => {
      if (disposed) return;
      if (event.kind === "error") {
        setError(event.message || t("Playback failed."));
        setControlsVisible(true);
      } else if (event.kind === "warning" && event.message) {
        console.warn("GStreamer playback warning:", event.message);
      }
      if (event.kind === "stateChanged" || event.kind === "buffering" || event.kind === "tracksChanged") {
        void refresh();
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch((reason) => setError(String(reason)));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refresh, t]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void playerWindow.listen<string>("native-player://startup-error", ({ payload }) => {
      if (!disposed) {
        setError(payload || t("Playback failed."));
        setControlsVisible(true);
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch((reason) => setError(String(reason)));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [playerWindow, t]);

  useEffect(() => () => {
    if (hideTimer.current != null) window.clearTimeout(hideTimer.current);
  }, []);

  useEffect(() => {
    if (state.status !== "playing" || optionsOpen) setControlsVisible(true);
  }, [optionsOpen, state.status]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, select, button")) return;
      if (event.code === "Space") {
        event.preventDefault();
        void run(stateRef.current.status === "playing" ? nativePlayerPause : nativePlayerPlay);
      } else if (event.key === "ArrowLeft") {
        event.preventDefault();
        seekRelative(-10_000);
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        seekRelative(10_000);
      } else if (event.key.toLowerCase() === "m") {
        void run(() => nativePlayerSetMuted(!stateRef.current.muted));
      } else if (event.key.toLowerCase() === "l") {
        void run(() => nativePlayerSetLooping(!stateRef.current.looping));
      } else if (event.key.toLowerCase() === "f") {
        void playerWindow.isFullscreen().then((fullscreen) => playerWindow.setFullscreen(!fullscreen));
      } else if (event.key === "Escape") {
        void playerWindow.isFullscreen().then((fullscreen) => {
          if (fullscreen) return playerWindow.setFullscreen(false);
        });
      }
      revealControls();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [playerWindow, revealControls, run, seekRelative]);

  const chooseExternalSubtitle = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: t("Subtitle files"), extensions: ["srt", "vtt", "ass", "ssa", "sub"] }],
    });
    if (typeof selected === "string") {
      await run(() => nativePlayerSetSubtitleUri(localPathToFileUri(selected)));
    }
  };

  const duration = state.durationMs || 0;
  const displayedPosition = seekDraft ?? Math.min(state.positionMs || 0, duration || Number.MAX_SAFE_INTEGER);
  const selectedAudio = state.audioTracks.find((track) => track.selected)?.index ?? -1;
  const selectedSubtitle = state.subtitleTracks.find((track) => track.selected)?.index ?? -1;
  const isPlaying = state.status === "playing" || state.status === "buffering";

  const commitSeek = () => {
    if (seekDraft == null) return;
    const target = seekDraft;
    setSeekDraft(null);
    void run(() => nativePlayerSeek(target));
  };

  const toggleFullscreen = async () => {
    await playerWindow.setFullscreen(!(await playerWindow.isFullscreen()));
  };

  return (
    <main
      className={`native-player-stage ${controlsVisible || optionsOpen ? "controls-visible" : "controls-hidden"}`}
      tabIndex={-1}
      onPointerMove={revealControls}
      onPointerLeave={hideControlsSoon}
      onDoubleClick={(event) => {
        if (event.target === event.currentTarget) void toggleFullscreen();
      }}
    >
      <header
        className="native-player-title-glass"
        onPointerEnter={keepControlsVisible}
        onPointerLeave={revealControls}
        onPointerDown={(event) => {
          if (event.button === 0 && !(event.target as HTMLElement).closest("button")) void playerWindow.startDragging();
        }}
      >
        <span className="native-player-title">{state.title || t("Native Media Player")}</span>
        {startupMessage && (
          <span className="native-player-buffering">{startupMessage}</span>
        )}
        {state.status === "buffering" && (
          <span className="native-player-buffering">{t("Buffering")} {state.bufferingPercent ?? 0}%</span>
        )}
        <button className="native-player-window-button" title={t("Fullscreen")} onClick={() => void toggleFullscreen()}><PlayerGlyph name="fullscreen" /></button>
        <button className="native-player-window-button close" title={t("Close")} onClick={() => void playerWindow.close()}><PlayerGlyph name="close" /></button>
      </header>

      {!isPlaying && state.status !== "stopped" && (
        <button
          className="native-player-center-play player-surface"
          aria-label={t("Play")}
          onClick={() => void run(nativePlayerPlay)}
        ><PlayerGlyph name="play" /></button>
      )}

      {optionsOpen && (
        <section
          className="native-player-options player-surface"
          onPointerEnter={keepControlsVisible}
          onPointerLeave={revealControls}
        >
          <label>
            <span>{t("Playback speed")}</span>
            <select value={state.rate} onChange={(event) => void run(() => nativePlayerSetRate(Number(event.target.value)))}>
              {RATES.map((rate) => <option key={rate} value={rate}>{rate}×</option>)}
            </select>
          </label>
          <label>
            <span>{t("Audio track")}</span>
            <select
              value={selectedAudio}
              disabled={!state.audioTracks.length}
              onChange={(event) => void run(() => nativePlayerSelectAudioTrack(Number(event.target.value)))}
            >
              {!state.audioTracks.length && <option value={-1}>{t("No audio tracks")}</option>}
              {state.audioTracks.map((track) => <option key={track.index} value={track.index}>{track.label}</option>)}
            </select>
          </label>
          <label>
            <span>{t("Embedded subtitles")}</span>
            <select value={selectedSubtitle} onChange={(event) => void run(() => nativePlayerSelectSubtitleTrack(Number(event.target.value)))}>
              <option value={-1}>{t("Off")}</option>
              {state.subtitleTracks.map((track) => <option key={track.index} value={track.index}>{track.label}</option>)}
            </select>
          </label>
          <div className="native-player-option-actions">
            <span>{t("External subtitles")}</span>
            <button onClick={() => void chooseExternalSubtitle()}>{t("Open subtitle")}</button>
            {state.externalSubtitleUri && <button onClick={() => void run(() => nativePlayerSetSubtitleUri(null))}>{t("Remove")}</button>}
          </div>
        </section>
      )}

      <section
        className="native-player-control-glass player-surface"
        onPointerEnter={keepControlsVisible}
        onPointerLeave={revealControls}
      >
        <div className="native-player-timeline-row">
          <span>{formatTime(displayedPosition)}</span>
          <input
            className="native-player-timeline"
            aria-label={t("Playback position")}
            type="range"
            min={0}
            max={Math.max(duration, 1)}
            value={displayedPosition}
            disabled={!duration}
            onChange={(event) => setSeekDraft(Number(event.target.value))}
            onPointerUp={commitSeek}
            onKeyUp={commitSeek}
          />
          <span>{formatTime(state.durationMs)}</span>
        </div>
        <div className="native-player-controls-row">
          <div className="native-player-control-cluster transport">
            <button title={t("Back 10 seconds")} onClick={() => seekRelative(-10_000)}><PlayerGlyph name="back" /><small>10</small></button>
            <button className="primary" aria-label={isPlaying ? t("Pause") : t("Play")} onClick={() => void run(isPlaying ? nativePlayerPause : nativePlayerPlay)}>
              <PlayerGlyph name={isPlaying ? "pause" : "play"} />
            </button>
            <button title={t("Forward 10 seconds")} onClick={() => seekRelative(10_000)}><PlayerGlyph name="forward" /><small>10</small></button>
          </div>
          <div className="native-player-control-cluster sound">
            <button className={state.muted ? "active" : ""} title={state.muted ? t("Unmute") : t("Mute")} onClick={() => void run(() => nativePlayerSetMuted(!state.muted))}>
              <PlayerGlyph name={state.muted ? "volumeOff" : "volume"} />
            </button>
          <input
            className="native-player-volume"
            aria-label={t("Volume")}
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={state.volume}
            onChange={(event) => {
              const volume = Number(event.target.value);
              setState((current) => ({ ...current, volume }));
              void nativePlayerSetVolume(volume).catch((reason) => setError(String(reason)));
            }}
            />
          </div>
          <span className="native-player-spacer" />
          <div className="native-player-control-cluster utilities">
            <button className={state.looping ? "active" : ""} title={t("Repeat")} onClick={() => void run(() => nativePlayerSetLooping(!state.looping))}><PlayerGlyph name="repeat" /></button>
            <button className={optionsOpen ? "active" : ""} title={t("Playback options")} onClick={() => setOptionsOpen((open) => !open)}><PlayerGlyph name="more" /></button>
            <button title={t("Fullscreen")} onClick={() => void toggleFullscreen()}><PlayerGlyph name="fullscreen" /></button>
          </div>
        </div>
      </section>

      {error && <div className="native-player-error player-surface" role="alert">{error}</div>}
    </main>
  );
}
