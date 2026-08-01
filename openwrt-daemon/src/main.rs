use openwrt_heriheri::VfsTree;
use openwrt_lanzou::{AppState, LanzouCloud};
use openwrt_lanzou_down::LanzouDownloader;

use std::collections::HashMap;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    println!("==========================================");
    println!(" HeriHeriCloud OpenWRT WebDAV Daemon");
    println!("==========================================");

    // 1. Read persistent config directly to bypass environment variables
    let config_json = std::fs::read_to_string("heriheri_config.json").unwrap_or_default();
    let config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));
    let phone = config["lanzou_phone"].as_str().unwrap_or("").to_string();
    let password = config["lanzou_pass"].as_str().unwrap_or("").to_string();

    let lanzou = Arc::new(Mutex::new(LanzouCloud::new()));
    let downloader = Arc::new(Mutex::new(LanzouDownloader::new()));
    let vfs_tree = Arc::new(Mutex::new(None));

    // 2. Perform Login ONLY if credentials exist in config
    if !phone.is_empty() && !password.is_empty() {
        println!("[AUTH] Logging into Lanzou Cloud with {}...", phone);
        let mut lanzou_guard = lanzou.lock().await;
        
        if let Err(e) = lanzou_guard.login(&phone, &password).await {
            println!("[AUTH] Login failed: {}", e);
        } else {
            println!("[AUTH] Login successful!");
            
            // 3. Initialize VFS if login succeeded
            println!("[VFS] Locating Virtual File System...");
            if let Ok((root_id, deeper_id)) = lanzou_guard.init_vfs_root().await {
                let file_name = format!("heriheri_tree_{}.txt", phone);
                let tree_path = std::path::PathBuf::from(format!("/tmp/{}", file_name));
                
                let tree = match VfsTree::load_local(tree_path.clone()) {
                    Ok(mut t) => {
                        if t.deeperdir_lanzou_id.is_empty() {
                            t.deeperdir_lanzou_id = deeper_id;
                            let _ = t.save_local();
                        }
                        println!("[VFS] Loaded existing local tree from RAM");
                        t
                    }
                    Err(_) => {
                        println!("[VFS] No local tree found in RAM. Creating fresh VFS...");
                        let t = VfsTree::new(root_id, deeper_id, tree_path);
                        let _ = t.save_local();
                        t
                    }
                };
                *vfs_tree.lock().await = Some(tree);
            }
        }
    } else {
        println!("[AUTH] No account configured! Please visit the WebGUI to sign in.");
    }

    // 4. Build the Application State
    let state = AppState {
        lanzou,
        downloader,
        vfs: vfs_tree,
        pid_stack: Arc::new(Mutex::new(vec![0])),
        task_ctrl: Arc::new(Mutex::new(HashMap::new())),
        sync_lock: Arc::new(Mutex::new(())),
        upload_limit: Arc::new(AtomicU32::new(0)),
        download_limit: Arc::new(AtomicU32::new(0)),
        current_phone: Arc::new(Mutex::new(phone)),
    };

    // 5. Perform Sync Pull if VFS was successfully initialized
    if state.vfs.lock().await.is_some() {
        println!("[SYNC] Pulling latest file tree from the cloud...");
        let _ = openwrt_lanzou::execute_sync_pull(&state).await;
    }

    // 6. Boot the WebDAV Proxy Server (Waits infinitely for WebUI or WebDAV clients)
    openwrt_webdav::run_server(state).await;
}