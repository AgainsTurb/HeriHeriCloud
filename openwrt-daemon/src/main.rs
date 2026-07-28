mod openwrt_heriheri;
mod openwrt_lanzou;
mod openwrt_lanzou_down;
mod openwrt_webdav;

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

    // 1. Read Lanzou credentials from Environment Variables
    let phone = std::env::var("HERI_PHONE").unwrap_or_else(|_| {
        println!("[FATAL] Please set the HERI_PHONE environment variable.");
        std::process::exit(1);
    });
    
    let password = std::env::var("HERI_PASS").unwrap_or_else(|_| {
        println!("[FATAL] Please set the HERI_PASS environment variable.");
        std::process::exit(1);
    });

    // 2. Initialize Core Cloud Services
    let lanzou = Arc::new(Mutex::new(LanzouCloud::new()));
    let downloader = Arc::new(Mutex::new(LanzouDownloader::new()));

    // 3. Perform Headless Login
    println!("[AUTH] Logging into Lanzou Cloud with {}...", phone);
    let mut lanzou_guard = lanzou.lock().await;
    if let Err(e) = lanzou_guard.login(&phone, &password).await {
        println!("[FATAL] Login failed: {}", e);
        std::process::exit(1);
    }
    println!("[AUTH] Login successful!");

    // 4. Initialize the VFS Root
    println!("[VFS] Locating Virtual File System...");
    let (root_id, deeper_id) = lanzou_guard.init_vfs_root().await.unwrap_or_else(|e| {
        println!("[FATAL] Could not initialize VFS Root: {}", e);
        std::process::exit(1);
    });
    drop(lanzou_guard); // Free the lock so other tasks can use LanzouCloud

    // 5. Create the VFS Tree in RAM
    // Note: OpenWRT's /tmp directory is mounted as RAMfs (tmpfs). 
    // Storing the DB here ensures we never burn out the router's fragile flash memory.
    let file_name = format!("heriheri_tree_{}.txt", phone);
    let tree_path = std::path::PathBuf::from(format!("/tmp/{}", file_name));
    
    let vfs_tree = match VfsTree::load_local(tree_path.clone()) {
        Ok(mut tree) => {
            if tree.deeperdir_lanzou_id.is_empty() {
                tree.deeperdir_lanzou_id = deeper_id;
                let _ = tree.save_local();
            }
            println!("[VFS] Loaded existing local tree from RAM (Timestamp: {})", tree.last_modified);
            tree
        }
        Err(_) => {
            println!("[VFS] No local tree found in RAM. Creating fresh VFS...");
            let tree = VfsTree::new(root_id, deeper_id, tree_path);
            let _ = tree.save_local();
            tree
        }
    };

    // 6. Build the Application State
    let state = AppState {
        lanzou,
        downloader,
        vfs: Arc::new(Mutex::new(Some(vfs_tree))),
        pid_stack: Arc::new(Mutex::new(vec![0])),
        task_ctrl: Arc::new(Mutex::new(HashMap::new())),
        sync_lock: Arc::new(Mutex::new(())),
        upload_limit: Arc::new(AtomicU32::new(0)),
        download_limit: Arc::new(AtomicU32::new(0)),
        current_phone: Arc::new(Mutex::new(phone)),
    };

    // 7. Perform Sync Pull (Download the latest movie database from Lanzou)
    println!("[SYNC] Pulling latest file tree from the cloud...");
    match openwrt_lanzou::execute_sync_pull(&state).await {
        Ok(true) => println!("[SYNC] File tree successfully updated!"),
        Ok(false) => println!("[SYNC] File tree is already up to date."),
        Err(e) => println!("[SYNC] Warning: Could not pull latest tree: {}", e),
    }

    // 8. Boot the WebDAV Proxy Server (Runs infinitely)
    openwrt_webdav::run_server(state).await;
}