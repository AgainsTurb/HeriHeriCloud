// swift-tools-version:5.9
import PackageDescription

let package = Package(
  name: "tauri-plugin-gstreamer-player",
  platforms: [
    .iOS(.v13),
    .macOS(.v10_15),
  ],
  products: [
    .library(
      name: "tauri-plugin-gstreamer-player",
      type: .static,
      targets: ["tauri-plugin-gstreamer-player"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .binaryTarget(
      name: "GStreamer",
      path: "Frameworks/GStreamer.xcframework"),
    .target(
      name: "GStreamerBridge",
      dependencies: ["GStreamer"],
      path: "Sources/GStreamerBridge",
      publicHeadersPath: "include"),
    .target(
      name: "tauri-plugin-gstreamer-player",
      dependencies: [
        .byName(name: "Tauri"),
        .byName(name: "GStreamerBridge")
      ],
      path: "Sources/Plugin")
  ]
)
