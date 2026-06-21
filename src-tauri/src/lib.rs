mod lanzou;
mod lanzou_down;
mod heriheri;
use lanzou::{AppState, LanzouCloud, set_lanzou_cookies, login, request_register_sms, submit_register, init_vfs_root, vfs_list_dir, vfs_enter_folder, 
    vfs_go_back, vfs_create_folder, vfs_upload_file, vfs_delete_item, vfs_get_share_info, vfs_control_task, vfs_get_current_pid, vfs_expand_drop, 
    vfs_get_breadcrumbs, vfs_batch_delete, vfs_move_items, vfs_list_bin, vfs_restore_items, vfs_hard_delete_items, vfs_rename_item, vfs_download_file,
    vfs_get_folder_tree, vfs_sync_push, vfs_sync_pull};
use tokio::sync::Mutex;
use std::collections::HashMap;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            lanzou: Mutex::new(LanzouCloud::new()),
            downloader: Mutex::new(crate::lanzou_down::LanzouDownloader::new()),
            vfs: Mutex::new(None),
            pid_stack: Mutex::new(Vec::new()),
            task_ctrl: Mutex::new(HashMap::new()),
            sync_lock: tokio::sync::Mutex::new(()),
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
            vfs_sync_pull
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}