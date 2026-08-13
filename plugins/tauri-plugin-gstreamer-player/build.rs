const COMMANDS: &[&str] = &[
    "open",
    "play",
    "pause",
    "stop",
    "seek",
    "set_volume",
    "set_muted",
    "set_rate",
    "set_looping",
    "select_audio_track",
    "select_subtitle_track",
    "set_subtitle_uri",
    "get_state",
    "capabilities",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .try_build()
        .expect("failed to build the GStreamer player plugin");
}
