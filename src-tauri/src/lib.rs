mod heriheri;
mod lanzou;
mod lanzou_down;
use lanzou::{
    init_vfs_root, login, request_register_sms, set_lanzou_cookies, submit_register,
    vfs_batch_delete, vfs_control_task, vfs_create_folder, vfs_delete_item, vfs_download_file,
    vfs_enter_folder, vfs_expand_drop, vfs_get_breadcrumbs, vfs_get_current_pid,
    vfs_get_folder_tree, vfs_get_share_info, vfs_go_back, vfs_hard_delete_items, vfs_list_bin,
    vfs_list_dir, vfs_move_items, vfs_rename_item, vfs_restore_items, vfs_sync_pull, vfs_sync_push,
    vfs_upload_file, AppState, LanzouCloud, vfs_update_speed_limits, vfs_generate_share_code,
    vfs_resolve_share_code, vfs_rent_item
};
use std::collections::HashMap;
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            lanzou: Mutex::new(LanzouCloud::new()),
            downloader: Mutex::new(crate::lanzou_down::LanzouDownloader::new()),
            vfs: Mutex::new(None),
            pid_stack: Mutex::new(Vec::new()),
            task_ctrl: Mutex::new(HashMap::new()),
            sync_lock: tokio::sync::Mutex::new(()),
            upload_limit: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            download_limit: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            current_phone: tokio::sync::Mutex::new(String::new()),
        })
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
            vfs_rent_item
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
