# HeriHeriCloud GStreamer Player

This local Tauri plugin provides one playback command surface on Windows, Ubuntu, macOS, Android, and iOS. The desktop implementation is Rust plus `gstreamer-rs`; Android and iOS use native GStreamer surfaces because mobile operating systems cannot launch a playback sidecar.

The public command namespace is `plugin:gstreamer-player|*`. Protocol version 2 supports transport, seeking, volume/mute, 0.25x–4x playback rates, repeat, embedded subtitle and audio-track selection, external subtitle URIs, state polling, and capability discovery. Frame-processor API version 1 is reserved; the only installed processor is currently `passthrough`, so ONNX requests fail explicitly instead of silently changing video quality.

On Windows, Ubuntu, and macOS, `playbin` is attached to the Tauri player window through GStreamer's `VideoOverlay` interface. The transparent WebView in that same window supplies auto-hiding liquid-glass controls over the native video surface. Destroying the player window is observed by the Rust plugin, which detaches the native handle and releases the pipeline independently of frontend callbacks. Android and iOS use equivalent floating native controls over their `SurfaceView`/`UIView` render surfaces.

## Platform SDK setup

`npm run tauri -- ...` bootstraps a missing GStreamer SDK before handing control to Tauri. The bootstrap dynamically resolves the newest stable upstream series (an even minor version), verifies the official SHA-256 checksum, and reuses its ignored download cache. Set `GSTREAMER_VERSION=x.y.z` only when a reproducible older release is intentionally required.

- Windows: selects MSVC `x86_64`, `arm64`, or `x86` from the Cargo target/host architecture and installs the official combined runtime plus development package under `.gstreamer/`. No administrator access is required.
- Ubuntu/Debian: installs the newest packages available from the configured distribution repositories, including development files, base/good/bad/ugly plugins, and `gst-libav`. It uses `sudo` when required. Fedora and Arch package managers are also recognized.
- macOS: downloads and checksum-verifies the official universal runtime and development packages, invokes the system installer through `sudo`, and supplies the framework's `bin` and `pkgconfig` paths to Cargo.
- Android: downloads the official universal SDK (a large archive), extracts it below `.gstreamer/android/`, and sets both `GSTREAMER_ROOT_ANDROID` and the Gradle property for the build. A valid caller-supplied `GSTREAMER_ROOT_ANDROID` remains authoritative. The native build includes the restricted codec group so `libav` can provide HEVC/H.265 fallback.
- iOS: on the required macOS host, downloads the official XCFramework, verifies it, and places it at ignored `ios/Frameworks/GStreamer.xcframework` for Swift Package Manager.

Manual preparation remains available through `npm run setup:gstreamer`, or the platform-specific `setup:gstreamer:*` scripts. These scripts prepare source development and local builds; shipping a self-contained installer to end users still requires bundling/deploying the GStreamer runtime with the application package.
