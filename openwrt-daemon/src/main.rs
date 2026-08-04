mod openwrt_heriheri;
mod openwrt_lanzou;
mod openwrt_lanzou_down;
mod openwrt_webdav;

use clap::{Parser, Subcommand};
use openwrt_heriheri::VfsTree;
use openwrt_lanzou::{AppState, LanzouCloud};
use openwrt_lanzou_down::LanzouDownloader;

use std::collections::HashMap;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use tokio::sync::Mutex;

// =====================================================================
// CLI ARGUMENT ROUTER
// =====================================================================
#[derive(Parser)]
#[command(name = "HeriHeri Core")]
#[command(about = "Universal CLI and WebDAV Daemon for HeriHeriCloud", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the WebDAV, MCP, and WebUI Daemon (Default)
    Daemon,

    /// Login to your Lanzou Cloud account
    Login { phone: String, password: String },

    /// Logout and clear saved credentials
    Logout,

    /// List files in the current or target directory
    Ls { #[arg(default_value = ".")] path: String },

    /// Change the current working directory
    Cd { #[arg(default_value = "/")] path: String },

    /// Print the current working directory
    Pwd,
    
    /// Upload local files to the cloud
    Upload { 
        #[arg(required = true, value_name = "本地文件... 目标目录", help = "本地文件路径，最后一个参数为云端目标目录")] 
        args: Vec<String>, 
    },
    
    /// Download files from the cloud
    Download { 
        #[arg(required = true, value_name = "云端文件... 本地目录", help = "云端文件路径，最后一个参数为本地保存目录")] 
        args: Vec<String>, 
    },
    
    /// Move files or folders to the recycle bin
    Rm { #[arg(required = true, value_name = "云端路径...")] paths: Vec<String> },
    
    /// Create a new folder
    Mkdir { path: String },
    
    /// Rename a file or folder
    Rename { path: String, new_name: String },
    
    /// Move items to another directory
    Mv { 
        #[arg(required = true, value_name = "源文件... 目标目录", help = "要移动的云端文件，最后一个参数为目标目录")] 
        args: Vec<String> 
    },
    
    /// List all items currently in the recycle bin
    Bin,

    /// Restore items from the recycle bin
    Restore { #[arg(required = true, value_name = "文件ID...")] vfs_ids: Vec<u64> },

    /// Permanently delete items from the recycle bin
    HardDelete { #[arg(required = true, value_name = "文件ID...")] vfs_ids: Vec<u64> },

    /// Generate a secure sharing link (heri://...) for a file
    Share { path: String },

    /// Save a shared file (heri://...) to your own cloud
    Rent { code: String, #[arg(short, long, default_value = ".")] target_path: String },

    /// Search the Virtual File System by name or MD5 hash
    Search { query: String },
    
    /// Force push local VFS changes to the cloud
    Sync,

    /// Switch CLI Language (en / zh)
    Lang { #[arg(value_name = "en|zh")] code: String },
}

async fn resolve_path(state: &AppState, cwd_id: u64, path: &str) -> Result<u64, String> {
    let vfs_guard = state.vfs.lock().await;
    let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;
    
    let path = path.trim();
    if path.is_empty() || path == "." { return Ok(cwd_id); }
    if path == "/" { return Ok(0); }
    
    let mut curr_id = if path.starts_with('/') { 0 } else { cwd_id };
    
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if segment == "." { continue; }
        if segment == ".." {
            if let Some(node) = tree.nodes.get(&curr_id) { curr_id = node.pid; } else { curr_id = 0; }
            continue;
        }
        
        let mut found = None;
        for node in tree.nodes.values() {
            if node.pid == curr_id && !node.is_deleted && !node.is_trashed && node.name == segment {
                found = Some(node.id);
                break;
            }
        }
        
        if let Some(id) = found {
            curr_id = id;
        } else {
            return Err(format!("'{}' not found", segment));
        }
    }
    Ok(curr_id)
}

async fn get_full_path(state: &AppState, id: u64) -> String {
    let vfs_guard = state.vfs.lock().await;
    if let Some(tree) = vfs_guard.as_ref() {
        let mut path = Vec::new();
        let mut curr = id;
        while let Some(n) = tree.nodes.get(&curr) {
            path.push(n.name.clone());
            curr = n.pid;
        }
        path.reverse();
        if path.is_empty() { "/".to_string() } else { format!("/{}", path.join("/")) }
    } else {
        "/".to_string()
    }
}

async fn execute_command(
    command: Option<Commands>, 
    state: &AppState, 
    cwd_file: &std::path::PathBuf, 
    cwd_id: &mut u64
) {
    match command {
        Some(Commands::Login { phone, password }) => {
            println!("[AUTH] Attempting to log in as {}...", phone);
            match openwrt_lanzou::login(phone.clone(), password.clone(), state).await {
                Ok(_) => {
                    let config_json = std::fs::read_to_string("heriheri_config.json").unwrap_or_default();
                    let mut config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));
                    config["lanzou_phone"] = serde_json::Value::String(phone.clone());
                    config["lanzou_pass"] = serde_json::Value::String(password.clone());
                    let _ = std::fs::write("heriheri_config.json", serde_json::to_string_pretty(&config).unwrap_or_default());

                    if let Err(e) = openwrt_lanzou::init_vfs_root(phone, state).await {
                        println!("[ERROR] VFS Init failed: {}", e);
                    } else {
                        let _ = openwrt_lanzou::vfs_sync_pull(state).await;
                        println!("\n[SUCCESS] Login and Sync complete!");
                    }
                }
                Err(e) => println!("[ERROR] Login failed: {}", e),
            }
        }
        Some(Commands::Logout) => {
            let config_json = std::fs::read_to_string("heriheri_config.json").unwrap_or_default();
            let mut config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));
            config["lanzou_phone"] = serde_json::Value::String("".to_string());
            config["lanzou_pass"] = serde_json::Value::String("".to_string());
            let _ = std::fs::write("heriheri_config.json", serde_json::to_string_pretty(&config).unwrap_or_default());
            let _ = std::fs::remove_file(cwd_file);
            *cwd_id = 0;
            println!("[SUCCESS] Logged out successfully.");
        }
        Some(Commands::Pwd) => {
            println!("{}", get_full_path(state, *cwd_id).await);
        }
        Some(Commands::Cd { path }) => {
            match resolve_path(state, *cwd_id, &path).await {
                Ok(pid) => {
                    let _ = std::fs::write(cwd_file, pid.to_string());
                    *cwd_id = pid; // Update memory state instantly
                }
                Err(e) => println!("[ERROR] {}", e),
            }
        }
        Some(Commands::Ls { path }) => {
            match resolve_path(state, *cwd_id, &path).await {
                Ok(pid) => {
                    let _ = openwrt_lanzou::vfs_enter_folder(pid, state).await;
                    if let Ok(nodes) = openwrt_lanzou::vfs_list_dir(state).await {
                        println!("\n{:<12} | {:<5} | {:<12} | {}", "VFS ID", "TYPE", "SIZE", "NAME");
                        println!("{:-<12}-+-{:-<5}-+-{:-<12}-+-{:-<40}", "", "", "", "");
                        for n in nodes {
                            println!("{:<12} | {:<5} | {:<12} | {}", n.id, n.node_type.as_str(), n.size, n.name);
                        }
                        println!();
                    }
                }
                Err(e) => println!("[ERROR] {}", e),
            }
        }
        Some(Commands::Upload { mut args }) => {
            let dest_path = if args.len() > 1 { args.pop().unwrap() } else { ".".to_string() };
            
            match resolve_path(state, *cwd_id, &dest_path).await {
                Ok(pid) => {
                    for file_path in args {
                        // Safely expand Unix HOME directory paths
                        let expanded_path = if file_path.starts_with("~/") || file_path == "~" {
                            file_path.replacen("~", &std::env::var("HOME").unwrap_or_default(), 1)
                        } else {
                            file_path
                        };
                        
                        let task_id = format!("cli_up_{}", crate::openwrt_heriheri::current_timestamp());
                        println!("\n[UPLOAD] Starting {}...", expanded_path);
                        match openwrt_lanzou::vfs_upload_file(expanded_path, task_id, pid, "".to_string(), 0, state).await {
                            Ok(_) => println!("[SUCCESS] Upload complete!"),
                            Err(e) => println!("[ERROR] Upload failed: {}", e),
                        }
                    }
                }
                Err(e) => println!("[ERROR] Invalid destination: {}", e),
            }
        }
        Some(Commands::Download { mut args }) => {
            // UNIX Behavior: Last argument is destination, everything else is VFS files to download
            let dest_path = if args.len() > 1 { args.pop().unwrap() } else { ".".to_string() };
            
            let expanded_dest = if dest_path.starts_with("~/") || dest_path == "~" {
                dest_path.replacen("~", &std::env::var("HOME").unwrap_or_default(), 1)
            } else {
                dest_path
            };

            // If downloading multiple files, OR destination already exists as a folder, append file name
            let is_dir_target = std::path::Path::new(&expanded_dest).is_dir() || args.len() > 1;

            for path in args {
                match resolve_path(state, *cwd_id, &path).await {
                    Ok(vfs_id) => {
                        let original_name = {
                            let vfs_guard = state.vfs.lock().await;
                            vfs_guard.as_ref().unwrap().nodes.get(&vfs_id).map(|n| n.name.clone()).unwrap_or("downloaded_file".to_string())
                        };
                        
                        let final_dest = if is_dir_target {
                            let target_dir = std::path::Path::new(&expanded_dest);
                            if !target_dir.exists() { std::fs::create_dir_all(target_dir).unwrap_or_default(); }
                            target_dir.join(original_name).to_string_lossy().to_string()
                        } else {
                            expanded_dest.clone()
                        };

                        let task_id = format!("cli_down_{}", crate::openwrt_heriheri::current_timestamp());
                        println!("\n[DOWNLOAD] Saving to {}...", final_dest);
                        match openwrt_lanzou::vfs_download_file(task_id, vfs_id, None, final_dest, 0, 0, state).await {
                            Ok(_) => println!("[SUCCESS] Download complete!"),
                            Err(e) => println!("[ERROR] Download failed: {}", e),
                        }
                    }
                    Err(e) => println!("[ERROR] File not found: {}", e),
                }
            }
        }
        Some(Commands::Rm { paths }) => {
            let mut ids = Vec::new();
            for p in paths {
                match resolve_path(state, *cwd_id, &p).await {
                    Ok(id) => ids.push(id),
                    Err(e) => println!("[ERROR] Skipping '{}': {}", p, e),
                }
            }
            if !ids.is_empty() {
                match openwrt_lanzou::vfs_batch_delete(ids, state).await {
                    Ok(_) => println!("[SUCCESS] Items moved to Trash Bin."),
                    Err(e) => println!("[ERROR] Delete failed: {}", e),
                }
            }
        }
        Some(Commands::Mkdir { path }) => {
            let p = std::path::Path::new(&path);
            let mut parent_str = p.parent().unwrap_or(std::path::Path::new("")).to_string_lossy().to_string();
            if parent_str.is_empty() { parent_str = ".".to_string(); }
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            
            match resolve_path(state, *cwd_id, &parent_str).await {
                Ok(pid) => {
                    let _ = openwrt_lanzou::vfs_enter_folder(pid, state).await;
                    match openwrt_lanzou::vfs_create_folder(name.clone(), "".to_string(), state).await {
                        Ok(_) => println!("[SUCCESS] Folder '{}' created successfully!", name),
                        Err(e) => println!("[ERROR] Failed to create folder: {}", e),
                    }
                }
                Err(e) => println!("[ERROR] Invalid parent path: {}", e),
            }
        }
        Some(Commands::Rename { path, new_name }) => {
            match resolve_path(state, *cwd_id, &path).await {
                Ok(vfs_id) => {
                    match openwrt_lanzou::vfs_rename_item(vfs_id, new_name.clone(), state).await {
                        Ok(_) => println!("[SUCCESS] Item renamed to '{}'", new_name),
                        Err(e) => println!("[ERROR] Rename failed: {}", e),
                    }
                }
                Err(e) => println!("[ERROR] Path not found: {}", e),
            }
        }
        Some(Commands::Mv { mut args }) => {
            let target = if args.len() > 1 { args.pop().unwrap() } else { ".".to_string() };
            match resolve_path(state, *cwd_id, &target).await {
                Ok(target_pid) => {
                    let mut ids = Vec::new();
                    for p in args {
                        match resolve_path(state, *cwd_id, &p).await {
                            Ok(id) => ids.push(id),
                            Err(e) => println!("[ERROR] Skipping '{}': {}", p, e),
                        }
                    }
                    if !ids.is_empty() {
                        match openwrt_lanzou::vfs_move_items(ids, target_pid, state).await {
                            Ok(_) => println!("[SUCCESS] Items moved to {}", target),
                            Err(e) => println!("[ERROR] Move failed: {}", e),
                        }
                    }
                }
                Err(e) => println!("[ERROR] Target path invalid: {}", e),
            }
        }
        Some(Commands::Bin) => {
            match openwrt_lanzou::vfs_list_bin(state).await {
                Ok(nodes) => {
                    println!("\n{:<12} | {:<5} | {:<12} | {}", "VFS ID", "TYPE", "SIZE", "NAME");
                    println!("{:-<12}-+-{:-<5}-+-{:-<12}-+-{:-<40}", "", "", "", "");
                    for n in nodes { println!("{:<12} | {:<5} | {:<12} | {}", n.id, n.node_type.as_str(), n.size, n.name); }
                    println!();
                }
                Err(e) => println!("[ERROR] Failed to list recycle bin: {}", e),
            }
        }
        Some(Commands::Restore { vfs_ids }) => {
            match openwrt_lanzou::vfs_restore_items(vfs_ids, state).await {
                Ok(_) => println!("[SUCCESS] Items restored successfully!"),
                Err(e) => println!("[ERROR] Restore failed: {}", e),
            }
        }
        Some(Commands::HardDelete { vfs_ids }) => {
            match openwrt_lanzou::vfs_hard_delete_items(vfs_ids, state).await {
                Ok(_) => println!("[SUCCESS] Items permanently deleted!"),
                Err(e) => println!("[ERROR] Hard delete failed: {}", e),
            }
        }
        Some(Commands::Share { path }) => {
            match resolve_path(state, *cwd_id, &path).await {
                Ok(id) => {
                    match openwrt_lanzou::vfs_generate_share_code(id, state).await {
                        Ok(code) => println!("\n[SHARE CODE]\n{}", code),
                        Err(e) => println!("[ERROR] Failed to generate share code: {}", e),
                    }
                }
                Err(e) => println!("[ERROR] Path not found: {}", e),
            }
        }
        Some(Commands::Rent { code, target_path }) => {
            match resolve_path(state, *cwd_id, &target_path).await {
                Ok(pid) => {
                    match openwrt_lanzou::vfs_rent_item(code, pid, state).await {
                        Ok(_) => println!("[SUCCESS] Shared item saved to your cloud!"),
                        Err(e) => println!("[ERROR] Failed to save shared item: {}", e),
                    }
                }
                Err(e) => println!("[ERROR] Target path invalid: {}", e),
            }
        }
        Some(Commands::Search { query }) => {
            match openwrt_lanzou::vfs_search(query, state).await {
                Ok(results) => {
                    println!("\n{:<12} | {}", "VFS ID", "PATH");
                    println!("{:-<12}-+-{:-<50}", "", "");
                    for r in results {
                        let id = r["id"].as_u64().unwrap_or(0);
                        let path = r["path_str"].as_str().unwrap_or("");
                        println!("{:<12} | {}", id, path);
                    }
                    println!();
                }
                Err(e) => println!("[ERROR] Search failed: {}", e),
            }
        }
        Some(Commands::Sync) => {
            match openwrt_lanzou::vfs_sync_push(state).await {
                Ok(_) => println!("[SUCCESS] Local tree forcefully pushed to cloud!"),
                Err(e) => println!("[ERROR] Sync failed: {}", e),
            }
        }
        Some(Commands::Lang { code }) => {
            let config_json = std::fs::read_to_string("heriheri_config.json").unwrap_or_default();
            let mut config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));
            
            if code.to_lowercase() == "zh" {
                config["cli_lang"] = serde_json::Value::String("zh".to_string());
                crate::LANG.store(1, std::sync::atomic::Ordering::Relaxed);
                println!("[SUCCESS] 已切换到中文");
            } else {
                config["cli_lang"] = serde_json::Value::String("en".to_string());
                crate::LANG.store(0, std::sync::atomic::Ordering::Relaxed);
                println!("[SUCCESS] Switched to English");
            }
            let _ = std::fs::write("heriheri_config.json", serde_json::to_string_pretty(&config).unwrap_or_default());
        }
        Some(Commands::Daemon) | None => {
            openwrt_webdav::run_server(state.clone()).await;
        }
    }
}

#[tokio::main]
async fn main() {
    let config_json = std::fs::read_to_string("heriheri_config.json").unwrap_or_default();
    let config: serde_json::Value = serde_json::from_str(&config_json).unwrap_or(serde_json::json!({}));

    if config["cli_lang"].as_str().unwrap_or("en") == "zh" {
        LANG.store(1, std::sync::atomic::Ordering::Relaxed);
    }

    let phone = config["lanzou_phone"].as_str().unwrap_or("").to_string();
    let password = config["lanzou_pass"].as_str().unwrap_or("").to_string();

    let lanzou = Arc::new(Mutex::new(LanzouCloud::new()));
    let downloader = Arc::new(Mutex::new(LanzouDownloader::new()));
    let vfs_tree = Arc::new(Mutex::new(None));

    if !phone.is_empty() && !password.is_empty() {
        let mut lanzou_guard = lanzou.lock().await;
        if lanzou_guard.login(&phone, &password).await.is_ok() {
            if let Ok((root_id, deeper_id)) = lanzou_guard.init_vfs_root().await {
                let file_name = format!("heriheri_tree_{}.txt", phone);
                let tree_path = std::path::PathBuf::from(format!("/tmp/{}", file_name));
                let tree = match VfsTree::load_local(tree_path.clone()) {
                    Ok(mut t) => {
                        if t.deeperdir_lanzou_id.is_empty() {
                            t.deeperdir_lanzou_id = deeper_id;
                            let _ = t.save_local();
                        }
                        t
                    }
                    Err(_) => {
                        let t = VfsTree::new(root_id, deeper_id, tree_path);
                        let _ = t.save_local();
                        t
                    }
                };
                *vfs_tree.lock().await = Some(tree);
            }
        }
    }

    let state = AppState {
        lanzou, downloader, vfs: vfs_tree,
        pid_stack: Arc::new(Mutex::new(vec![0])),
        task_ctrl: Arc::new(Mutex::new(HashMap::new())),
        sync_lock: Arc::new(Mutex::new(())),
        upload_limit: Arc::new(AtomicU32::new(0)),
        download_limit: Arc::new(AtomicU32::new(0)),
        current_phone: Arc::new(Mutex::new(phone)),
    };

    if state.vfs.lock().await.is_some() {
        let _ = openwrt_lanzou::execute_sync_pull(&state).await;
    }

    let cwd_file = std::env::temp_dir().join("heriheri_cwd.txt");
    let mut cwd_id: u64 = std::fs::read_to_string(&cwd_file).unwrap_or_default().trim().parse().unwrap_or(0);

    let args: Vec<String> = std::env::args().collect();
    
    if args.len() > 1 {
        let cli = Cli::parse();
        execute_command(cli.command, &state, &cwd_file, &mut cwd_id).await;
    } else {
        println!("==========================================");
        println!(" HeriHeriCloud 命令行交互模式已启动 ");
        println!("==========================================");
        println!("输入 'help' 查看所有命令。输入 'exit' 退出。\n");

        let history_file = std::env::temp_dir().join("heriheri_history.txt");
        let mut rl = rustyline::DefaultEditor::new().expect("Failed to initialize terminal");
        let _ = rl.load_history(&history_file);

        loop {
            let path_str = get_full_path(&state, cwd_id).await;
            let prompt = format!("heriheri:{} > ", path_str);
            
            match rl.readline(&prompt) {
                Ok(line) => {
                    let input = line.trim();
                    if input.is_empty() { continue; }
                    if input == "exit" || input == "quit" { 
                        println!("Goodbye!");
                        break; 
                    }

                    // Save to up-arrow history
                    let _ = rl.add_history_entry(input);
                    let _ = rl.save_history(&history_file);

                    if let Some(mut parsed_args) = shlex::split(input) {
                        let mut cmd_args = vec!["heriheri-cli".to_string()];
                        cmd_args.append(&mut parsed_args);
                        
                        match Cli::try_parse_from(cmd_args) {
                            Ok(cli) => execute_command(cli.command, &state, &cwd_file, &mut cwd_id).await,
                            Err(e) => { let _ = e.print(); }
                        }
                    } else {
                        println!("[ERROR] Unmatched quotes in your command.");
                    }
                }
                Err(rustyline::error::ReadlineError::Interrupted) | 
                Err(rustyline::error::ReadlineError::Eof) => {
                    println!("Goodbye!");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }
    }
}