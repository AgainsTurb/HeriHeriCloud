use serde::{Deserialize, Serialize};

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

pub const PLUGIN_NAME: &str = "gstreamer-player";
pub const PROTOCOL_VERSION: u32 = 2;
pub const FRAME_PROCESSOR_API_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub uri: String,
    pub title: String,
    #[serde(default)]
    pub is_audio: bool,
    #[serde(default)]
    pub start_position_ms: Option<u64>,
    #[serde(default)]
    pub renderer_window_label: Option<String>,
    #[serde(default)]
    pub controller_window_label: Option<String>,
    #[serde(default)]
    pub processor: FrameProcessorRequest,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FrameProcessorRequest {
    #[default]
    Passthrough,
    Onnx {
        model_id: String,
        operation: AiOperation,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AiOperation {
    SuperResolution,
    FrameInterpolation,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResponse {
    pub generation: u64,
    pub renderer_mode: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStateSnapshot {
    pub generation: u64,
    pub status: &'static str,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub looping: bool,
    pub buffering_percent: Option<i32>,
    pub title: Option<String>,
    pub audio_tracks: Vec<MediaTrack>,
    pub subtitle_tracks: Vec<MediaTrack>,
    pub external_subtitle_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTrack {
    pub index: i32,
    pub label: String,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerCapabilities {
    pub protocol_version: u32,
    pub frame_processor_api_version: u32,
    pub engine: &'static str,
    pub native_video: bool,
    pub playback_rates: Vec<f64>,
    pub embedded_subtitles: bool,
    pub external_subtitles: bool,
    pub multiple_audio_tracks: bool,
    pub ai_processors: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerEvent {
    pub generation: u64,
    pub kind: &'static str,
    pub message: Option<String>,
    pub percent: Option<i32>,
}

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    #[cfg(desktop)]
    {
        desktop::init()
    }
    #[cfg(mobile)]
    {
        mobile::init()
    }
}
