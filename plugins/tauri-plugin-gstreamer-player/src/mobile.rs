#![cfg(mobile)]

use tauri::{plugin::Builder, Manager, Runtime};

use crate::PLUGIN_NAME;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.heriheri.gstreamerplayer";

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_gstreamer_player);

pub struct GStreamerPlayer<R: Runtime>(#[allow(dead_code)] tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    Builder::new(PLUGIN_NAME)
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "GStreamerPlayerPlugin")?;
            #[cfg(target_os = "ios")]
            let handle = api.register_ios_plugin(init_plugin_gstreamer_player)?;
            app.manage(GStreamerPlayer(handle));
            Ok(())
        })
        .build()
}
