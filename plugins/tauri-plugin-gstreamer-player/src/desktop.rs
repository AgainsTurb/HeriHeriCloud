use std::sync::{
    atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use gstreamer as gst;
use gstreamer_video::{prelude::*, VideoOverlay};
use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle};
use tauri::{plugin::Builder, AppHandle, Emitter, Manager, RunEvent, Runtime, State, WindowEvent};

use crate::{
    FrameProcessorRequest, MediaTrack, OpenRequest, OpenResponse, PlayerCapabilities, PlayerEvent,
    PlayerStateSnapshot, FRAME_PROCESSOR_API_VERSION, PLUGIN_NAME, PROTOCOL_VERSION,
};

const EVENT_NAME: &str = "gstreamer-player://event";
const MIN_RATE: f64 = 0.25;
const MAX_RATE: f64 = 4.0;

struct Player {
    pipeline: Option<gst::Element>,
    video_overlay: Option<VideoOverlay>,
    renderer_window_label: Option<String>,
    controller_window_label: Option<String>,
    generation: Arc<AtomicU64>,
    desired_playing: Arc<AtomicBool>,
    looping: Arc<AtomicBool>,
    rate_bits: Arc<AtomicU64>,
    buffering_percent: Arc<AtomicI32>,
    volume: f64,
    muted: bool,
    rate: f64,
    title: Option<String>,
    external_subtitle_uri: Option<String>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            pipeline: None,
            video_overlay: None,
            renderer_window_label: None,
            controller_window_label: None,
            generation: Arc::new(AtomicU64::new(0)),
            desired_playing: Arc::new(AtomicBool::new(false)),
            looping: Arc::new(AtomicBool::new(false)),
            rate_bits: Arc::new(AtomicU64::new(1.0_f64.to_bits())),
            buffering_percent: Arc::new(AtomicI32::new(-1)),
            volume: 1.0,
            muted: false,
            rate: 1.0,
            title: None,
            external_subtitle_uri: None,
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(pipeline) = self.pipeline.take() {
            let _ = pipeline.set_state(gst::State::Null);
        }
        if let Some(overlay) = self.video_overlay.take() {
            unsafe { overlay.set_window_handle(0) };
        }
    }
}

#[derive(Default)]
struct PlayerStore(Mutex<Player>, Mutex<()>);

fn lock_player<'a>(
    store: &'a State<'a, PlayerStore>,
) -> Result<std::sync::MutexGuard<'a, Player>, String> {
    store
        .0
        .lock()
        .map_err(|_| "GStreamer player state is unavailable".to_string())
}

fn validate_uri(uri: &str) -> Result<(), String> {
    if uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("file://") {
        Ok(())
    } else {
        Err("Only HTTP(S) and file media sources are accepted".to_string())
    }
}

fn renderer_handle<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<usize, String> {
    let window = app
        .get_window(label)
        .ok_or_else(|| format!("The player window '{label}' does not exist"))?;

    const HANDLE_ATTEMPTS: usize = 20;
    const HANDLE_RETRY_DELAY: Duration = Duration::from_millis(10);
    for attempt in 1..=HANDLE_ATTEMPTS {
        match window.window_handle() {
            Ok(window_handle) => {
                return match window_handle.as_raw() {
                    RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as usize),
                    RawWindowHandle::AppKit(handle) => Ok(handle.ns_view.as_ptr() as usize),
                    RawWindowHandle::Xlib(handle) => Ok(handle.window as usize),
                    RawWindowHandle::Xcb(handle) => Ok(handle.window.get() as usize),
                    RawWindowHandle::Wayland(handle) => Ok(handle.surface.as_ptr() as usize),
                    other => Err(format!("Unsupported desktop window handle: {other:?}")),
                };
            }
            Err(HandleError::Unavailable) if attempt < HANDLE_ATTEMPTS => {
                std::thread::sleep(HANDLE_RETRY_DELAY);
            }
            Err(error) => {
                return Err(format!(
                    "Unable to access the player window handle after {attempt} attempt(s): {error}"
                ));
            }
        }
    }
    unreachable!("the window-handle retry loop always returns")
}

fn attach_renderer(pipeline: &gst::Element, window_handle: usize) -> Result<(), String> {
    let bus = pipeline
        .bus()
        .ok_or("The GStreamer playback pipeline has no message bus")?;

    // playbin is a controller/pipeline, not the concrete video sink, and therefore cannot be
    // cast to VideoOverlay. The sink is selected asynchronously once typefinding completes. Its
    // synchronous prepare-window-handle message is the only reliable point at which to attach an
    // application-owned surface before the sink creates its own top-level window.
    bus.set_sync_handler(move |_, message| {
        if !gstreamer_video::is_video_overlay_prepare_window_handle_message(message) {
            return gst::BusSyncReply::Pass;
        }

        let Some(source) = message.src() else {
            return gst::BusSyncReply::Pass;
        };
        let Some(overlay) = source.dynamic_cast_ref::<VideoOverlay>() else {
            return gst::BusSyncReply::Pass;
        };
        overlay.handle_events(false);
        unsafe { overlay.set_window_handle(window_handle) };
        gst::BusSyncReply::Drop
    });
    Ok(())
}

fn release_pipeline(player: &mut Player) -> Result<(), String> {
    player.generation.fetch_add(1, Ordering::AcqRel);
    player.desired_playing.store(false, Ordering::Release);
    let stop_result = if let Some(pipeline) = player.pipeline.take() {
        pipeline
            .set_state(gst::State::Null)
            .map(|_| ())
            .map_err(|error| format!("Unable to stop playback: {error}"))
    } else {
        Ok(())
    };
    if let Some(overlay) = player.video_overlay.take() {
        unsafe { overlay.set_window_handle(0) };
    }
    player.renderer_window_label = None;
    player.controller_window_label = None;
    stop_result
}

fn build_pipeline(request: &OpenRequest, volume: f64, muted: bool) -> Result<gst::Element, String> {
    gst::init().map_err(|error| format!("Unable to initialize GStreamer: {error}"))?;

    if !matches!(request.processor, FrameProcessorRequest::Passthrough) {
        return Err(
            "The ONNX frame-processor API is reserved but no AI processor is installed".to_string(),
        );
    }

    let processor_slot = gst::ElementFactory::make("identity")
        .name("heriheri-frame-processor-slot")
        .build()
        .map_err(|error| format!("Unable to create the frame-processor slot: {error}"))?;

    let playbin = gst::ElementFactory::make("playbin")
        .name("heriheri-player")
        .property("uri", &request.uri)
        .property("volume", volume)
        .property("mute", muted)
        .property("force-aspect-ratio", true)
        .build()
        .map_err(|error| format!("Unable to create the GStreamer playbin: {error}"))?;

    playbin.set_property("video-filter", &processor_slot);
    Ok(playbin)
}

fn watch_bus<R: Runtime>(
    app: AppHandle<R>,
    pipeline: gst::Element,
    generation_counter: Arc<AtomicU64>,
    desired_playing: Arc<AtomicBool>,
    looping: Arc<AtomicBool>,
    rate_bits: Arc<AtomicU64>,
    buffering_percent: Arc<AtomicI32>,
    generation: u64,
) {
    let Some(bus) = pipeline.bus() else {
        return;
    };

    std::thread::Builder::new()
        .name(format!("heriheri-gstreamer-bus-{generation}"))
        .spawn(move || {
            while generation_counter.load(Ordering::Acquire) == generation {
                let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(250)) else {
                    continue;
                };

                let event = match message.view() {
                    gst::MessageView::Eos(..) if looping.load(Ordering::Acquire) => {
                        let rate = f64::from_bits(rate_bits.load(Ordering::Acquire));
                        if seek_to(&pipeline, rate, gst::ClockTime::ZERO).is_ok()
                            && desired_playing.load(Ordering::Acquire)
                        {
                            let _ = pipeline.set_state(gst::State::Playing);
                        }
                        Some(PlayerEvent {
                            generation,
                            kind: "looped",
                            message: None,
                            percent: None,
                        })
                    }
                    gst::MessageView::Eos(..) => {
                        desired_playing.store(false, Ordering::Release);
                        Some(PlayerEvent {
                            generation,
                            kind: "ended",
                            message: None,
                            percent: None,
                        })
                    }
                    gst::MessageView::Error(error) => {
                        desired_playing.store(false, Ordering::Release);
                        Some(PlayerEvent {
                            generation,
                            kind: "error",
                            message: Some(match error.debug() {
                                Some(debug) => format!("{} ({debug})", error.error()),
                                None => error.error().to_string(),
                            }),
                            percent: None,
                        })
                    }
                    gst::MessageView::Warning(warning) => Some(PlayerEvent {
                        generation,
                        kind: "warning",
                        message: Some(warning.error().to_string()),
                        percent: None,
                    }),
                    gst::MessageView::Buffering(buffering) => {
                        let percent = buffering.percent();
                        buffering_percent.store(percent, Ordering::Release);
                        if percent < 100 {
                            let _ = pipeline.set_state(gst::State::Paused);
                        } else if desired_playing.load(Ordering::Acquire) {
                            let _ = pipeline.set_state(gst::State::Playing);
                        }
                        Some(PlayerEvent {
                            generation,
                            kind: "buffering",
                            message: None,
                            percent: Some(percent),
                        })
                    }
                    gst::MessageView::StateChanged(change)
                        if change.src().is_some_and(|source| {
                            source == pipeline.upcast_ref::<gst::Object>()
                        }) =>
                    {
                        Some(PlayerEvent {
                            generation,
                            kind: "stateChanged",
                            message: Some(format!("{:?}", change.current()).to_lowercase()),
                            percent: None,
                        })
                    }
                    gst::MessageView::StreamsSelected(..)
                    | gst::MessageView::StreamCollection(..) => Some(PlayerEvent {
                        generation,
                        kind: "tracksChanged",
                        message: None,
                        percent: None,
                    }),
                    _ => None,
                };

                if let Some(event) = event {
                    let terminal = event.kind == "error";
                    if terminal {
                        eprintln!(
                            "[HeriHeriCloud GStreamer] Playback pipeline error: {}",
                            event
                                .message
                                .as_deref()
                                .unwrap_or("unknown GStreamer error")
                        );
                    }
                    let _ = app.emit(EVENT_NAME, event);
                    if terminal {
                        break;
                    }
                }
            }
        })
        .ok();
}

fn seek_to(pipeline: &gst::Element, rate: f64, position: gst::ClockTime) -> Result<(), String> {
    pipeline
        .seek(
            rate,
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::SeekType::Set,
            position,
            gst::SeekType::None,
            gst::ClockTime::NONE,
        )
        .map_err(|error| format!("GStreamer rejected playback rate {rate}: {error}"))
}

fn string_tag(tags: &gst::TagList, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        tags.generic(*name)
            .and_then(|value| value.get::<String>().ok())
            .filter(|value| !value.trim().is_empty())
    })
}

fn collect_tracks(
    playbin: &gst::Element,
    count_property: &str,
    current_property: &str,
    tag_signal: &str,
    fallback: &str,
) -> Vec<MediaTrack> {
    let count = playbin.property::<i32>(count_property).max(0);
    let selected = playbin.property::<i32>(current_property);
    (0..count)
        .map(|index| {
            let tags = playbin.emit_by_name::<Option<gst::TagList>>(tag_signal, &[&index]);
            let language = tags
                .as_ref()
                .and_then(|list| string_tag(list, &["language-name", "language-code"]));
            let codec = tags.as_ref().and_then(|list| {
                string_tag(
                    list,
                    &["audio-codec", "subtitle-codec", "video-codec", "codec"],
                )
            });
            let title = tags
                .as_ref()
                .and_then(|list| string_tag(list, &["title", "description"]));
            let label = title
                .or_else(|| language.clone())
                .or_else(|| codec.clone())
                .unwrap_or_else(|| format!("{fallback} {}", index + 1));
            MediaTrack {
                index,
                label,
                language,
                codec,
                selected: index == selected,
            }
        })
        .collect()
}

// Raw window-handle lookup dispatches through Tauri's event loop. This command must not
// execute on that same thread or the synchronous dispatcher reports HandleError::Unavailable.
#[tauri::command(async)]
fn open<R: Runtime>(
    app: AppHandle<R>,
    store: State<'_, PlayerStore>,
    request: OpenRequest,
) -> Result<OpenResponse, String> {
    let result = open_inner(app, store, request);
    if let Err(error) = &result {
        eprintln!("[HeriHeriCloud GStreamer] Unable to open native playback: {error}");
    }
    result
}

fn open_inner<R: Runtime>(
    app: AppHandle<R>,
    store: State<'_, PlayerStore>,
    request: OpenRequest,
) -> Result<OpenResponse, String> {
    eprintln!(
        "[HeriHeriCloud GStreamer] Opening '{}' from {}",
        request.title, request.uri
    );
    validate_uri(&request.uri)?;
    let _open_guard = store
        .1
        .lock()
        .map_err(|_| "GStreamer startup coordinator is unavailable".to_string())?;
    let renderer_label = request
        .renderer_window_label
        .as_deref()
        .ok_or("Desktop playback requires a renderer window label")?;

    // Pipeline construction initializes GStreamer and inspects the plugin registry. Do not hold
    // the shared player mutex while that work runs; otherwise controller state polling and close
    // commands block behind startup and make the window appear frozen.
    let (volume, muted, generation) = {
        let mut player = lock_player(&store)?;
        release_pipeline(&mut player)?;
        let generation = player.generation.fetch_add(1, Ordering::AcqRel) + 1;
        player.renderer_window_label = Some(renderer_label.to_string());
        player.controller_window_label = request.controller_window_label.clone();
        player.title = Some(request.title.clone());
        (player.volume, player.muted, generation)
    };
    let native_handle = match renderer_handle(&app, renderer_label) {
        Ok(handle) => handle,
        Err(error) => {
            let mut player = lock_player(&store)?;
            if player.generation.load(Ordering::Acquire) == generation {
                player.renderer_window_label = None;
                player.controller_window_label = None;
                player.title = None;
            }
            return Err(error);
        }
    };
    eprintln!("[HeriHeriCloud GStreamer] Native renderer handle is ready");
    let pipeline = match build_pipeline(&request, volume, muted) {
        Ok(pipeline) => pipeline,
        Err(error) => {
            let mut player = lock_player(&store)?;
            if player.generation.load(Ordering::Acquire) == generation {
                player.renderer_window_label = None;
                player.controller_window_label = None;
                player.title = None;
            }
            return Err(error);
        }
    };
    eprintln!("[HeriHeriCloud GStreamer] Decoder pipeline is ready");
    if app.get_window(renderer_label).is_none()
        || request
            .controller_window_label
            .as_deref()
            .is_some_and(|label| app.get_window(label).is_none())
    {
        let _ = pipeline.set_state(gst::State::Null);
        let mut player = lock_player(&store)?;
        if player.generation.load(Ordering::Acquire) == generation {
            player.renderer_window_label = None;
            player.controller_window_label = None;
            player.title = None;
        }
        return Err("The player window was closed during decoder startup".to_string());
    }
    if let Err(error) = attach_renderer(&pipeline, native_handle) {
        let _ = pipeline.set_state(gst::State::Null);
        let mut player = lock_player(&store)?;
        if player.generation.load(Ordering::Acquire) == generation {
            player.renderer_window_label = None;
            player.controller_window_label = None;
            player.title = None;
        }
        return Err(error);
    }
    eprintln!("[HeriHeriCloud GStreamer] Renderer attachment is ready");

    let mut player = lock_player(&store)?;
    if player.generation.load(Ordering::Acquire) != generation {
        let _ = pipeline.set_state(gst::State::Null);
        return Err("Native playback startup was cancelled".to_string());
    }
    player.rate = 1.0;
    player.rate_bits.store(1.0_f64.to_bits(), Ordering::Release);
    player.external_subtitle_uri = None;
    // The concrete sink is created later by playbin and receives the native surface through the
    // bus sync handler installed above. The pipeline owns that sink for its whole lifetime.
    player.video_overlay = None;
    player.desired_playing.store(true, Ordering::Release);
    player.buffering_percent.store(-1, Ordering::Release);

    watch_bus(
        app,
        pipeline.clone(),
        Arc::clone(&player.generation),
        Arc::clone(&player.desired_playing),
        Arc::clone(&player.looping),
        Arc::clone(&player.rate_bits),
        Arc::clone(&player.buffering_percent),
        generation,
    );

    if let Err(error) = pipeline.set_state(gst::State::Playing) {
        let _ = pipeline.set_state(gst::State::Null);
        if let Some(overlay) = player.video_overlay.take() {
            unsafe { overlay.set_window_handle(0) };
        }
        player.generation.fetch_add(1, Ordering::AcqRel);
        player.desired_playing.store(false, Ordering::Release);
        player.renderer_window_label = None;
        player.controller_window_label = None;
        player.title = None;
        return Err(format!("Unable to start GStreamer playback: {error}"));
    }

    if let Some(position_ms) = request.start_position_ms {
        if let Err(error) = pipeline.seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
            gst::ClockTime::from_mseconds(position_ms),
        ) {
            let _ = pipeline.set_state(gst::State::Null);
            if let Some(overlay) = player.video_overlay.take() {
                unsafe { overlay.set_window_handle(0) };
            }
            player.generation.fetch_add(1, Ordering::AcqRel);
            player.desired_playing.store(false, Ordering::Release);
            player.renderer_window_label = None;
            player.controller_window_label = None;
            player.title = None;
            return Err(format!(
                "Unable to seek to the requested start position: {error}"
            ));
        }
    }

    player.pipeline = Some(pipeline);
    eprintln!("[HeriHeriCloud GStreamer] Playback started");
    Ok(OpenResponse {
        generation,
        renderer_mode: "embedded-native-surface",
    })
}

#[tauri::command]
fn play(store: State<'_, PlayerStore>) -> Result<(), String> {
    let player = lock_player(&store)?;
    let pipeline = player.pipeline.as_ref().ok_or("No media is open")?;
    if let (Some(position), Some(duration)) = (
        pipeline.query_position::<gst::ClockTime>(),
        pipeline.query_duration::<gst::ClockTime>(),
    ) {
        if position.saturating_add(gst::ClockTime::from_mseconds(250)) >= duration {
            seek_to(pipeline, player.rate, gst::ClockTime::ZERO)?;
        }
    }
    player.desired_playing.store(true, Ordering::Release);
    pipeline
        .set_state(gst::State::Playing)
        .map(|_| ())
        .map_err(|error| format!("Unable to resume playback: {error}"))
}

#[tauri::command]
fn pause(store: State<'_, PlayerStore>) -> Result<(), String> {
    let player = lock_player(&store)?;
    let pipeline = player.pipeline.as_ref().ok_or("No media is open")?;
    player.desired_playing.store(false, Ordering::Release);
    pipeline
        .set_state(gst::State::Paused)
        .map(|_| ())
        .map_err(|error| format!("Unable to pause playback: {error}"))
}

#[tauri::command]
fn stop(store: State<'_, PlayerStore>) -> Result<(), String> {
    let mut player = lock_player(&store)?;
    release_pipeline(&mut player)?;
    player.title = None;
    player.external_subtitle_uri = None;
    player.rate = 1.0;
    Ok(())
}

#[tauri::command]
fn seek(store: State<'_, PlayerStore>, position_ms: u64) -> Result<(), String> {
    let player = lock_player(&store)?;
    let pipeline = player.pipeline.as_ref().ok_or("No media is open")?;
    seek_to(
        pipeline,
        player.rate,
        gst::ClockTime::from_mseconds(position_ms),
    )
    .map_err(|error| format!("GStreamer rejected the seek request: {error}"))
}

#[tauri::command]
fn set_volume(store: State<'_, PlayerStore>, volume: f64) -> Result<(), String> {
    if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
        return Err("Volume must be between 0 and 1".to_string());
    }
    let mut player = lock_player(&store)?;
    player.volume = volume;
    if let Some(pipeline) = player.pipeline.as_ref() {
        pipeline.set_property("volume", volume);
    }
    Ok(())
}

#[tauri::command]
fn set_muted(store: State<'_, PlayerStore>, muted: bool) -> Result<(), String> {
    let mut player = lock_player(&store)?;
    player.muted = muted;
    if let Some(pipeline) = player.pipeline.as_ref() {
        pipeline.set_property("mute", muted);
    }
    Ok(())
}

#[tauri::command]
fn set_rate(store: State<'_, PlayerStore>, rate: f64) -> Result<(), String> {
    if !rate.is_finite() || !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err(format!(
            "Playback rate must be between {MIN_RATE} and {MAX_RATE}"
        ));
    }
    let mut player = lock_player(&store)?;
    let pipeline = player.pipeline.as_ref().ok_or("No media is open")?;
    let position = pipeline
        .query_position::<gst::ClockTime>()
        .unwrap_or(gst::ClockTime::ZERO);
    seek_to(pipeline, rate, position)?;
    player.rate = rate;
    player.rate_bits.store(rate.to_bits(), Ordering::Release);
    Ok(())
}

#[tauri::command]
fn set_looping(store: State<'_, PlayerStore>, looping: bool) -> Result<(), String> {
    let player = lock_player(&store)?;
    player.looping.store(looping, Ordering::Release);
    Ok(())
}

#[tauri::command]
fn select_audio_track(store: State<'_, PlayerStore>, index: i32) -> Result<(), String> {
    let player = lock_player(&store)?;
    let pipeline = player.pipeline.as_ref().ok_or("No media is open")?;
    let count = pipeline.property::<i32>("n-audio");
    if index < 0 || index >= count {
        return Err("Audio track index is out of range".to_string());
    }
    pipeline.set_property("current-audio", index);
    Ok(())
}

#[tauri::command]
fn select_subtitle_track(store: State<'_, PlayerStore>, index: i32) -> Result<(), String> {
    let player = lock_player(&store)?;
    let pipeline = player.pipeline.as_ref().ok_or("No media is open")?;
    let count = pipeline.property::<i32>("n-text");
    if index < -1 || index >= count {
        return Err("Subtitle track index is out of range".to_string());
    }
    pipeline.set_property("current-text", index);
    Ok(())
}

#[tauri::command]
fn set_subtitle_uri(store: State<'_, PlayerStore>, uri: Option<String>) -> Result<(), String> {
    if let Some(value) = uri.as_deref() {
        validate_uri(value)?;
    }
    let mut player = lock_player(&store)?;
    let pipeline = player.pipeline.as_ref().ok_or("No media is open")?;
    pipeline.set_property("suburi", uri.as_deref());
    player.external_subtitle_uri = uri;
    Ok(())
}

#[tauri::command]
fn get_state(store: State<'_, PlayerStore>) -> Result<PlayerStateSnapshot, String> {
    let player = lock_player(&store)?;
    let (status, position_ms, duration_ms, audio_tracks, subtitle_tracks) =
        match player.pipeline.as_ref() {
            Some(pipeline) => {
                let status = match pipeline.current_state() {
                    gst::State::Playing => {
                        if player.desired_playing.load(Ordering::Acquire) {
                            "playing"
                        } else {
                            "paused"
                        }
                    }
                    gst::State::Paused => {
                        if player.desired_playing.load(Ordering::Acquire)
                            && player.buffering_percent.load(Ordering::Acquire) < 100
                        {
                            "buffering"
                        } else {
                            "paused"
                        }
                    }
                    gst::State::Ready => "ready",
                    gst::State::Null => "stopped",
                    _ => "changing",
                };
                (
                    status,
                    pipeline
                        .query_position::<gst::ClockTime>()
                        .map(|value| value.mseconds()),
                    pipeline
                        .query_duration::<gst::ClockTime>()
                        .map(|value| value.mseconds()),
                    collect_tracks(
                        pipeline,
                        "n-audio",
                        "current-audio",
                        "get-audio-tags",
                        "Audio",
                    ),
                    collect_tracks(
                        pipeline,
                        "n-text",
                        "current-text",
                        "get-text-tags",
                        "Subtitle",
                    ),
                )
            }
            None => ("stopped", None, None, Vec::new(), Vec::new()),
        };
    let buffering = player.buffering_percent.load(Ordering::Acquire);

    Ok(PlayerStateSnapshot {
        generation: player.generation.load(Ordering::Acquire),
        status,
        position_ms,
        duration_ms,
        volume: player.volume,
        muted: player.muted,
        rate: player.rate,
        looping: player.looping.load(Ordering::Acquire),
        buffering_percent: (0..100).contains(&buffering).then_some(buffering),
        title: player.title.clone(),
        audio_tracks,
        subtitle_tracks,
        external_subtitle_uri: player.external_subtitle_uri.clone(),
    })
}

#[tauri::command]
fn capabilities() -> PlayerCapabilities {
    PlayerCapabilities {
        protocol_version: PROTOCOL_VERSION,
        frame_processor_api_version: FRAME_PROCESSOR_API_VERSION,
        engine: "GStreamer",
        native_video: true,
        playback_rates: vec![0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0],
        embedded_subtitles: true,
        external_subtitles: true,
        multiple_audio_tracks: true,
        ai_processors: vec!["passthrough"],
    }
}

fn configure_bundled_runtime<R: Runtime>(app: &AppHandle<R>) {
    let Ok(resources) = app.path().resource_dir() else {
        return;
    };
    let plugins = resources.join("gstreamer-1.0");
    if !plugins.is_dir() {
        return;
    }

    std::env::set_var("GST_PLUGIN_SYSTEM_PATH_1_0", &plugins);
    std::env::set_var("GST_PLUGIN_PATH_1_0", &plugins);

    #[cfg(target_os = "windows")]
    let scanner = resources.join("gst-plugin-scanner.exe");
    #[cfg(target_os = "macos")]
    let scanner = resources
        .parent()
        .map(|contents| contents.join("Helpers").join("gst-plugin-scanner"));

    #[cfg(target_os = "windows")]
    if scanner.is_file() {
        std::env::set_var("GST_PLUGIN_SCANNER_1_0", scanner);
    }
    #[cfg(target_os = "macos")]
    if let Some(scanner) = scanner.filter(|path| path.is_file()) {
        std::env::set_var("GST_PLUGIN_SCANNER_1_0", scanner);
    }
}

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    Builder::new(PLUGIN_NAME)
        .setup(|app, _api| {
            configure_bundled_runtime(app);
            app.manage(PlayerStore::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open,
            play,
            pause,
            stop,
            seek,
            set_volume,
            set_muted,
            set_rate,
            set_looping,
            select_audio_track,
            select_subtitle_track,
            set_subtitle_uri,
            get_state,
            capabilities
        ])
        .on_event(|app, event| {
            let RunEvent::WindowEvent {
                label,
                event: WindowEvent::Destroyed,
                ..
            } = event
            else {
                return;
            };
            let Some(store) = app.try_state::<PlayerStore>() else {
                return;
            };
            let Ok(mut player) = store.0.lock() else {
                return;
            };
            if player.renderer_window_label.as_deref() != Some(label.as_str())
                && player.controller_window_label.as_deref() != Some(label.as_str())
            {
                return;
            }
            player.generation.fetch_add(1, Ordering::AcqRel);
            player.desired_playing.store(false, Ordering::Release);
            let paired_window_label =
                if player.renderer_window_label.as_deref() == Some(label.as_str()) {
                    player.controller_window_label.clone()
                } else {
                    player.renderer_window_label.clone()
                };
            player.renderer_window_label = None;
            player.controller_window_label = None;
            player.title = None;
            player.external_subtitle_uri = None;
            player.rate = 1.0;
            let pipeline = player.pipeline.take();
            let overlay = player.video_overlay.take();
            drop(player);
            let app = app.clone();
            let _ = std::thread::Builder::new()
                .name("heriheri-gstreamer-close".to_string())
                .spawn(move || {
                    if let Some(pipeline) = pipeline {
                        let _ = pipeline.set_state(gst::State::Null);
                    }
                    if let Some(overlay) = overlay {
                        unsafe { overlay.set_window_handle(0) };
                    }
                    if let Some(label) = paired_window_label {
                        if let Some(window) = app.get_window(&label) {
                            let _ = window.destroy();
                        }
                    }
                });
        })
        .build()
}
