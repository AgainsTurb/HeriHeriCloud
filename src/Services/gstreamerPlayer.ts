import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Window } from "@tauri-apps/api/window";
import { platform } from "@tauri-apps/plugin-os";

const COMMAND_PREFIX = "plugin:gstreamer-player|";
const STARTUP_STATUS_KEY = "heriheri_native_player_startup";

interface PlayerStartupStatus {
  nodeId: number;
  title: string;
  phase: "preparing" | "opening" | "ready" | "error";
  message?: string;
  updatedAt: number;
}

function publishStartupStatus(status: Omit<PlayerStartupStatus, "updatedAt">): void {
  localStorage.setItem(STARTUP_STATUS_KEY, JSON.stringify({ ...status, updatedAt: Date.now() }));
}

export function readNativePlayerStartupStatus(): PlayerStartupStatus | null {
  try {
    return JSON.parse(localStorage.getItem(STARTUP_STATUS_KEY) || "null") as PlayerStartupStatus | null;
  } catch {
    return null;
  }
}

export const GSTREAMER_MEDIA_EXTENSIONS = new Set([
  "mp4", "mkv", "webm", "mov", "m4v", "avi", "mpeg", "mpg", "ts", "m2ts",
  "ogv", "ogg", "mp3", "wav", "flac", "m4a", "aac", "opus", "wma",
]);

export type FrameProcessorRequest =
  | { kind: "passthrough" }
  | {
      kind: "onnx";
      modelId: string;
      operation: "superResolution" | "frameInterpolation";
    };

export interface NativePlayerState {
  generation: number;
  status: "stopped" | "ready" | "paused" | "playing" | "buffering" | "changing" | "open";
  positionMs: number | null;
  durationMs: number | null;
  volume: number;
  muted: boolean;
  rate: number;
  looping: boolean;
  bufferingPercent: number | null;
  title: string | null;
  audioTracks: MediaTrack[];
  subtitleTracks: MediaTrack[];
  externalSubtitleUri: string | null;
}

export interface MediaTrack {
  index: number;
  label: string;
  language: string | null;
  codec: string | null;
  selected: boolean;
}

export interface NativePlayerCapabilities {
  protocolVersion: number;
  frameProcessorApiVersion: number;
  engine: string;
  nativeVideo: boolean;
  playbackRates: number[];
  embeddedSubtitles: boolean;
  externalSubtitles: boolean;
  multipleAudioTracks: boolean;
  aiProcessors: string[];
}

export interface NativePlayerEvent {
  generation: number;
  kind: "looped" | "ended" | "error" | "warning" | "buffering" | "stateChanged" | "tracksChanged";
  message: string | null;
  percent: number | null;
}

interface OpenResponse {
  generation: number;
  rendererMode: "embedded-native-surface" | "native-surface";
}

export const DESKTOP_PLAYER_LABEL = "gstreamer-media-player";
export const DESKTOP_VIDEO_HOST_LABEL = "gstreamer-video-host";

async function waitForWindowCreation<T extends Window>(window: T): Promise<T> {
  await new Promise<void>((resolve, reject) => {
    let settled = false;
    const finish = (operation: () => void) => {
      if (settled) return;
      settled = true;
      globalThis.clearTimeout(timeout);
      operation();
    };
    const timeout = globalThis.setTimeout(
      () => finish(() => reject(new Error(`Timed out while creating window '${window.label}'`))),
      8_000,
    );
    void window.once("tauri://created", () => finish(resolve));
    void window.once<unknown>("tauri://error", (event) => finish(() => reject(event.payload)));
  });
  return window;
}

async function createDesktopVideoHostWindow(): Promise<Window> {
  return waitForWindowCreation(new Window(DESKTOP_VIDEO_HOST_LABEL, {
    title: "HeriHeriCloud Video",
    width: 1120,
    height: 700,
    minWidth: 720,
    minHeight: 440,
    center: true,
    resizable: false,
    decorations: false,
    focusable: false,
    skipTaskbar: true,
    visible: false,
    shadow: true,
    backgroundColor: [0, 0, 0, 255],
  }));
}

async function createDesktopPlayerWindow(title: string, videoHost: Window, isMacOS: boolean): Promise<WebviewWindow> {
  return waitForWindowCreation(new WebviewWindow(DESKTOP_PLAYER_LABEL, {
    url: "index.html#/native-player",
    title,
    width: 1120,
    height: 700,
    minWidth: 720,
    minHeight: 440,
    center: true,
    resizable: true,
    decorations: false,
    transparent: true,
    backgroundColor: [0, 0, 0, 0],
    visible: false,
    // The native video host already owns the window shadow. A second shadow on the transparent
    // child is composited as horizontal black seams by macOS WindowServer.
    shadow: !isMacOS,
    parent: videoHost,
  }));
}

async function alignVideoHost(controller: WebviewWindow, videoHost: Window): Promise<void> {
  const [position, size] = await Promise.all([controller.outerPosition(), controller.innerSize()]);
  await videoHost.setPosition(position);
  await videoHost.setSize(size);
}

export function getDesktopVideoHostWindow(): Promise<Window | null> {
  return Window.getByLabel(DESKTOP_VIDEO_HOST_LABEL);
}

interface LocalServiceConfig {
  webdavPort?: number;
  webdavUser?: string;
  webdavPass?: string;
}

function configuredLocalService(): Required<LocalServiceConfig> {
  try {
    const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}") as LocalServiceConfig;
    const port = Number(config.webdavPort);
    return {
      webdavPort: Number.isInteger(port) && port > 0 && port <= 65535 ? port : 8888,
      webdavUser: config.webdavUser || "admin",
      webdavPass: config.webdavPass || "admin",
    };
  } catch {
    // A malformed legacy setting should not prevent media fallback.
  }
  return { webdavPort: 8888, webdavUser: "admin", webdavPass: "admin" };
}

async function ensureLocalStreamServer(): Promise<number> {
  const config = configuredLocalService();
  return withTimeout(invoke<number>("boot_webdav_server", {
    port: config.webdavPort,
    username: config.webdavUser,
    password: config.webdavPass,
  }), 8_000, "The local WebDAV/media server did not become ready within 8 seconds");
}

async function withTimeout<T>(operation: Promise<T>, milliseconds: number, message: string): Promise<T> {
  let timeout: number | undefined;
  try {
    return await Promise.race([
      operation,
      new Promise<T>((_, reject) => {
        timeout = globalThis.setTimeout(() => reject(new Error(message)), milliseconds);
      }),
    ]);
  } finally {
    if (timeout !== undefined) globalThis.clearTimeout(timeout);
  }
}

export function isGStreamerMedia(name: string): boolean {
  const extension = name.split(".").pop()?.toLowerCase() || "";
  return GSTREAMER_MEDIA_EXTENSIONS.has(extension);
}

export async function openGStreamerMedia(
  node: { id: number; name: string },
  processor: FrameProcessorRequest = { kind: "passthrough" },
): Promise<void> {
  publishStartupStatus({ nodeId: node.id, title: node.name, phase: "preparing" });
  const currentPlatform = platform();
  let desktopWindow: WebviewWindow | null = null;
  let videoHost: Window | null = null;
  try {
    // Create the desktop controller first so cloud/server startup is observable and the user can
    // cancel it. Previously the server command ran before this try block and before any window was
    // shown, making a bind failure or slow startup look like an ignored double-click.
    if (currentPlatform !== "android" && currentPlatform !== "ios") {
      desktopWindow = await WebviewWindow.getByLabel(DESKTOP_PLAYER_LABEL);
      videoHost = await Window.getByLabel(DESKTOP_VIDEO_HOST_LABEL);
      if ((desktopWindow && !videoHost) || (!desktopWindow && videoHost)) {
        await desktopWindow?.destroy().catch(() => undefined);
        await videoHost?.destroy().catch(() => undefined);
        desktopWindow = null;
        videoHost = null;
      }
      if (!videoHost) {
        videoHost = await createDesktopVideoHostWindow();
      }
      if (!desktopWindow) {
        desktopWindow = await createDesktopPlayerWindow(node.name, videoHost, currentPlatform === "macos");
      }
      await alignVideoHost(desktopWindow, videoHost);
      // Surface the controller before GStreamer performs network typefinding. This keeps startup
      // observable and gives the user a working close button even when the media source is slow.
      await desktopWindow.setTitle(node.name);
      await videoHost.show();
      await desktopWindow.show();
      await desktopWindow.setFocus();
    }
    // Playback and WebDAV share the same Axum listener. Awaiting this command guarantees that the
    // socket is actually bound; it also reports bind failures instead of handing GStreamer a dead
    // loopback URL and waiting indefinitely for typefinding.
    const streamPort = await ensureLocalStreamServer();
    const uri = await invoke<string>("prepare_media_stream", {
      vfsId: node.id,
      port: streamPort,
    });
    if (desktopWindow && !(await WebviewWindow.getByLabel(DESKTOP_PLAYER_LABEL))) {
      await videoHost?.destroy().catch(() => undefined);
      return; // The user closed the controller while the cloud stream was being prepared.
    }
    publishStartupStatus({ nodeId: node.id, title: node.name, phase: "opening" });
    await invoke<OpenResponse>(`${COMMAND_PREFIX}open`, {
      request: {
        uri,
        title: node.name,
        isAudio: /\.(mp3|wav|flac|m4a|aac|opus|wma|ogg)$/i.test(node.name),
        processor,
        rendererWindowLabel: videoHost?.label,
        controllerWindowLabel: desktopWindow?.label,
      },
    });
    publishStartupStatus({ nodeId: node.id, title: node.name, phase: "ready" });

  } catch (error) {
    publishStartupStatus({
      nodeId: node.id,
      title: node.name,
      phase: "error",
      message: String(error),
    });
    const activeController = desktopWindow
      ? await WebviewWindow.getByLabel(DESKTOP_PLAYER_LABEL).catch(() => null)
      : null;
    if (desktopWindow && activeController) {
      await desktopWindow.emit("native-player://startup-error", String(error)).catch(() => undefined);
      await desktopWindow.show().catch(() => undefined);
      await desktopWindow.setFocus().catch(() => undefined);
    } else if (videoHost) {
      await videoHost.destroy().catch(() => undefined);
    }
    console.error("Native GStreamer playback failed.", error);
    throw new Error(`Native GStreamer playback failed: ${String(error)}`);
  }
}

export function nativePlayerPlay(): Promise<void> {
  return invoke(`${COMMAND_PREFIX}play`);
}

export function nativePlayerPause(): Promise<void> {
  return invoke(`${COMMAND_PREFIX}pause`);
}

export function nativePlayerStop(): Promise<void> {
  return invoke(`${COMMAND_PREFIX}stop`);
}

export function nativePlayerSeek(positionMs: number): Promise<void> {
  return invoke(`${COMMAND_PREFIX}seek`, { positionMs: Math.max(0, Math.round(positionMs)) });
}

export function nativePlayerSetVolume(volume: number): Promise<void> {
  return invoke(`${COMMAND_PREFIX}set_volume`, { volume: Math.min(1, Math.max(0, volume)) });
}

export function nativePlayerSetMuted(muted: boolean): Promise<void> {
  return invoke(`${COMMAND_PREFIX}set_muted`, { muted });
}

export function nativePlayerSetRate(rate: number): Promise<void> {
  return invoke(`${COMMAND_PREFIX}set_rate`, { rate });
}

export function nativePlayerSetLooping(looping: boolean): Promise<void> {
  return invoke(`${COMMAND_PREFIX}set_looping`, { looping });
}

export function nativePlayerSelectAudioTrack(index: number): Promise<void> {
  return invoke(`${COMMAND_PREFIX}select_audio_track`, { index });
}

export function nativePlayerSelectSubtitleTrack(index: number): Promise<void> {
  return invoke(`${COMMAND_PREFIX}select_subtitle_track`, { index });
}

export function nativePlayerSetSubtitleUri(uri: string | null): Promise<void> {
  return invoke(`${COMMAND_PREFIX}set_subtitle_uri`, { uri });
}

export function localPathToFileUri(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  if (normalized.startsWith("//")) {
    const [host, ...segments] = normalized.slice(2).split("/");
    return `file://${host}/${segments.map(encodeURIComponent).join("/")}`;
  }
  if (/^[A-Za-z]:\//.test(normalized)) {
    const drive = normalized.slice(0, 2);
    const segments = normalized.slice(3).split("/").map(encodeURIComponent);
    return `file:///${drive}/${segments.join("/")}`;
  }
  const absolute = normalized.startsWith("/") ? normalized : `/${normalized}`;
  return `file://${absolute.split("/").map(encodeURIComponent).join("/")}`;
}

export function getNativePlayerState(): Promise<NativePlayerState> {
  return invoke(`${COMMAND_PREFIX}get_state`);
}

export function getNativePlayerCapabilities(): Promise<NativePlayerCapabilities> {
  return invoke(`${COMMAND_PREFIX}capabilities`);
}

export function listenNativePlayerEvents(
  handler: (event: NativePlayerEvent) => void,
): Promise<UnlistenFn> {
  return listen<NativePlayerEvent>("gstreamer-player://event", ({ payload }) => handler(payload));
}
