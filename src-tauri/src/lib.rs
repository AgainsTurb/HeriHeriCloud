mod heriheri;
mod lanzou;
mod lanzou_down;
mod webdav;
use lanzou::{
    init_vfs_root, login, request_register_sms, set_lanzou_cookies, submit_register,
    vfs_batch_delete, vfs_control_task, vfs_create_folder, vfs_delete_item, vfs_download_file,
    vfs_enter_folder, vfs_expand_drop, vfs_generate_share_code, vfs_get_breadcrumbs,
    vfs_get_current_pid, vfs_get_folder_tree, vfs_get_share_info, vfs_go_back,
    vfs_hard_delete_items, vfs_list_bin, vfs_list_dir, vfs_move_items, vfs_rename_item,
    vfs_rent_item, vfs_resolve_share_code, vfs_restore_items, vfs_search, vfs_sync_pull,
    vfs_sync_push, vfs_update_speed_limits, vfs_upload_file, AppState, LanzouCloud,
};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;
use webdav::{boot_webdav_server, get_local_ip, get_webdav_config, set_webdav_config};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState {
        lanzou: Arc::new(Mutex::new(LanzouCloud::new())),
        downloader: Arc::new(Mutex::new(crate::lanzou_down::LanzouDownloader::new())),
        vfs: Arc::new(Mutex::new(None)),
        pid_stack: Arc::new(Mutex::new(Vec::new())),
        task_ctrl: Arc::new(Mutex::new(HashMap::new())),
        sync_lock: Arc::new(tokio::sync::Mutex::new(())),
        upload_limit: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        download_limit: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        current_phone: Arc::new(tokio::sync::Mutex::new(String::new())),
    };

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_keep_screen_on::init())
        .plugin(tauri_plugin_fs::init());

    #[cfg(mobile)]
    {
        builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    }

    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .manage(app_state)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            set_lanzou_cookies,
            login,
            request_register_sms,
            submit_register,
            init_vfs_root,
            vfs_list_dir,
            vfs_enter_folder,
            vfs_go_back,
            vfs_create_folder,
            vfs_upload_file,
            vfs_delete_item,
            vfs_get_share_info,
            vfs_control_task,
            vfs_get_current_pid,
            vfs_expand_drop,
            vfs_get_breadcrumbs,
            vfs_batch_delete,
            vfs_move_items,
            vfs_list_bin,
            vfs_restore_items,
            vfs_hard_delete_items,
            vfs_rename_item,
            vfs_download_file,
            vfs_get_folder_tree,
            vfs_sync_push,
            vfs_sync_pull,
            vfs_update_speed_limits,
            vfs_generate_share_code,
            vfs_resolve_share_code,
            vfs_rent_item,
            vfs_search,
            get_webdav_config,
            set_webdav_config,
            boot_webdav_server,
            get_local_ip
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
