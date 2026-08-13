# HeriHeriCloud GStreamer Player

This local Tauri plugin provides one playback command surface on Windows, Ubuntu, macOS, Android, and iOS. The desktop implementation is Rust plus `gstreamer-rs`; Android and iOS use native GStreamer surfaces because mobile operating systems cannot launch a playback sidecar.

The public command namespace is `plugin:gstreamer-player|*`. Protocol version 2 supports transport, seeking, volume/mute, 0.25x–4x playback rates, repeat, embedded subtitle and audio-track selection, external subtitle URIs, state polling, and capability discovery. Frame-processor API version 1 is reserved; the only installed processor is currently `passthrough`, so ONNX requests fail explicitly instead of silently changing video quality.

On Windows, Ubuntu, and macOS, `playbin` is attached to the Tauri player window through GStreamer's `VideoOverlay` interface. The transparent WebView in that same window supplies auto-hiding liquid-glass controls over the native video surface. Destroying the player window is observed by the Rust plugin, which detaches the native handle and releases the pipeline independently of frontend callbacks. Android and iOS use equivalent floating native controls over their `SurfaceView`/`UIView` render surfaces.

## Platform SDK locations

- Windows: run `npm run setup:gstreamer:windows`. The official MSVC runtime and development files are installed under ignored `.gstreamer/`; launch development/build commands through `npm run tauri:gstreamer -- ...` so the process can locate them.
- Ubuntu: install GStreamer development packages, base/good/bad/ugly plugins, and `gst-libav` through the distribution package manager. Build with the normal Tauri command after `pkg-config` can find `gstreamer-1.0`.
- macOS: install the official universal runtime and development packages. Ensure their `bin` directory and `pkgconfig` directories are visible to the build environment.
- Android: set `GSTREAMER_ROOT_ANDROID` to the extracted official universal Android SDK. The plugin builds `arm64-v8a` and `x86_64` native libraries through `ndk-build`.
- iOS: extract the official XCFramework to `ios/Frameworks/GStreamer.xcframework` before generating or building the iOS project.

No Android or Apple SDK is downloaded by the Windows setup script.
