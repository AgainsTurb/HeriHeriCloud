use axum::http::{header, StatusCode};
use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use axum::{Json, routing::post};
use rust_embed::RustEmbed;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use futures_util::stream::BoxStream;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

// Updated to point to your new headless modules
use crate::openwrt_lanzou::{AppState, SharePayload};

#[derive(RustEmbed)]
#[folder = "webui/"]
struct WebUiAssets;

const CHUNK_SIZE: usize = 100 * 1024 * 1024;
const PREFETCH_CLAMP: usize = 2 * 1024 * 1024;

static SERVER_STARTED: AtomicBool = AtomicBool::new(false);

// ========================================================
// MEMORY CACHE ENGINE
// ========================================================
#[derive(Clone)]
struct CachedMedia {
    chunks_str: String,
    total_size: usize,
    urls: Vec<String>,
    expires_at: Instant,
}

static URL_CACHE: std::sync::OnceLock<Arc<Mutex<HashMap<u64, CachedMedia>>>> =
    std::sync::OnceLock::new();

fn get_cache() -> Arc<Mutex<HashMap<u64, CachedMedia>>> {
    URL_CACHE
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

// ========================================================
// WEBDAV CONFIGURATION
// ========================================================
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct WebdavConfig {
    pub port: u16,
    pub username: String,
    pub password: String,
    #[serde(default)] pub lanzou_phone: String,
    #[serde(default)] pub lanzou_pass: String,
}

static WEBDAV_CONFIG: std::sync::OnceLock<Arc<tokio::sync::Mutex<WebdavConfig>>> =
    std::sync::OnceLock::new();

pub fn get_config() -> Arc<tokio::sync::Mutex<WebdavConfig>> {
    WEBDAV_CONFIG
        .get_or_init(|| {
            let default_cfg = WebdavConfig {
                port: 8888,
                username: "admin".to_string(),
                password: "admin".to_string(),
                lanzou_phone: "".to_string(),
                lanzou_pass: "".to_string(),
            };
            let cfg = std::fs::read_to_string("heriheri_config.json")
                .and_then(|s| serde_json::from_str(&s).map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "parse error")))
                .unwrap_or(default_cfg);
                
            Arc::new(tokio::sync::Mutex::new(cfg))
        })
        .clone()
}

fn get_filename_cipher() -> ChaCha20Poly1305 {
    let secret = std::env::var("CHUNKS_NAMES_KEY")
        .unwrap_or_else(|_| env!("HERIHERI_SECRET_KEY").to_string());

    let mut key_bytes = [0u8; 32];
    let bytes = secret.as_bytes();
    let len = std::cmp::min(bytes.len(), 32);
    key_bytes[..len].copy_from_slice(&bytes[..len]);

    let key = Key::from_slice(&key_bytes);
    ChaCha20Poly1305::new(key)
}

pub fn encrypt_chunk_filename(md5_str: &str, chunk_index: u32) -> String {
    let cipher = get_filename_cipher();
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let plaintext = format!("{}{:04x}", md5_str, chunk_index);
    let ciphertext = cipher.encrypt(&nonce, plaintext.as_bytes()).expect("Crypto Fail");

    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&ciphertext);

    hex::encode(payload)
}

pub fn decrypt_chunk_filename(filename: &str) -> Option<u32> {
    let base_name = filename.strip_suffix(".zip").unwrap_or(filename);
    let decoded = hex::decode(base_name).ok()?;

    if decoded.len() < 12 {
        return None;
    }

    let (nonce_bytes, ciphertext) = decoded.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = get_filename_cipher();

    let plaintext_bytes = cipher.decrypt(nonce, ciphertext).ok()?;
    let plaintext = String::from_utf8(plaintext_bytes).ok()?;

    if plaintext.len() >= 4 {
        let hex_idx = &plaintext[plaintext.len() - 4..];
        return u32::from_str_radix(hex_idx, 16).ok();
    }
    None
}

// ========================================================
// HELPER FUNCTIONS
// ========================================================
fn parse_size_to_bytes(s: &str) -> u64 {
    let s = s.to_uppercase().replace(" ", "");
    if s.is_empty() || s == "-" {
        return 0;
    }

    let mut num_str = String::new();
    let mut unit = "";
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            num_str.push(c);
        } else {
            unit = &s[num_str.len()..];
            break;
        }
    }
    let val = num_str.parse::<f64>().unwrap_or(0.0);
    let multiplier = match unit {
        "K" | "KB" => 1024.0,
        "M" | "MB" => 1024.0 * 1024.0,
        "G" | "GB" => 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    (val * multiplier) as u64
}

fn decode_url(input: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().unwrap_or('0');
            let h2 = chars.next().unwrap_or('0');
            if let Ok(b) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                bytes.push(b);
            }
        } else if c == '+' {
            bytes.push(b' ');
        } else {
            let mut buf = [0; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                bytes.push(b);
            }
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|_| input.to_string())
}

fn quick_xml_escape(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}

fn url_encode_segment(input: &str) -> String {
    const SAFE: &[u8] = b"-_.~";
    let mut encoded = String::with_capacity(input.len() * 3);
    for &byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || SAFE.contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{:02X}", byte));
        }
    }
    encoded
}

// ========================================================
// SERVER INITIALIZATION
// ========================================================
async fn fallback_logger(method: axum::http::Method, uri: axum::http::Uri) -> impl IntoResponse {
    println!(
        "\n[WEBDAV-FALLBACK] ⚠️ UNHANDLED PHANTOM PROBE: {} {}",
        method, uri
    );
    (StatusCode::NOT_FOUND, "Not Found")
}

pub async fn run_server(state: AppState) {
    let shared_state = Arc::new(state);

    let app_router = Router::new()
        // --- WEB UI & APIs ---
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .route("/api/status", get(api_status))
        .route("/api/login", post(api_login))
        .route("/api/sync", post(api_sync))
        .route("/api/logout", post(api_logout))
        .route("/api/config", get(api_get_config).post(api_set_config))
        
        // --- WEBDAV ROUTES ---
        .route("/stream/:vfs_id", get(handle_stream))
        .route("/dav", axum::routing::any(handle_dav_dispatch))
        .route("/dav/", axum::routing::any(handle_dav_dispatch))
        .route("/dav/*path", axum::routing::any(handle_dav_dispatch))
        
        // --- STATIC ASSETS CATCH-ALL ---
        .route("/:path", get(serve_assets))
        
        .fallback(fallback_logger)
        .with_state(shared_state);

    let config_arc = get_config();
    let config = config_arc.lock().await.clone();
    let port = config.port;

    let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            println!("\n[FATAL] Cannot bind to port {}: {}", port, e);
            println!("[FATAL] Another HeriHeri instance is already running in the background!");
            std::process::exit(1);
        }
    };
    println!("[PROXY] WebGUI available at http://127.0.0.1:{}", port);
    println!("[PROXY] WebDAV Mount available at http://127.0.0.1:{}/dav", port);

    axum::serve(listener, app_router).await.unwrap();
}

async fn resolve_lanzou_media(
    vfs_id: u64,
    state: &AppState,
) -> Result<(String, String, Option<String>, usize), String> {
    let vfs_guard = state.vfs.lock().await;
    let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;
    let node = tree.nodes.get(&vfs_id).cloned().ok_or("Node not found")?;

    let total_size = parse_size_to_bytes(&node.size) as usize;

    if node.lanzou_id.starts_with("alien://") {
        let encoded = node.lanzou_id.replace("alien://", "");
        let json_str = crate::openwrt_lanzou::decrypt_payload(&encoded)
            .map_err(|_| "Failed to decrypt Alien payload".to_string())?;
        let payload: SharePayload =
            serde_json::from_str(&json_str).map_err(|_| "Failed to parse JSON".to_string())?;
        Ok((
            node.chunks,
            payload.l,
            Some(payload.p).filter(|p| !p.is_empty()),
            total_size,
        ))
    } else {
        let is_folder = node.node_type == crate::openwrt_heriheri::NodeType::Directory
            || (node.chunks != "1" && !node.chunks.is_empty());
        let lanzou = state.lanzou.lock().await;
        let share_info = lanzou
            .get_share_info(node.lanzou_id.clone(), is_folder)
            .await?;

        let url = if let Some(u) = share_info["new_url"].as_str() {
            u.to_string()
        } else {
            format!(
                "{}/{}",
                share_info["is_newd"].as_str().unwrap_or(""),
                share_info["f_id"].as_str().unwrap_or("")
            )
        };

        let pwd = share_info["pwd"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(|s| s.to_string());
        if url.is_empty() || url == "/" {
            return Err("Could not get share URL".to_string());
        }
        Ok((node.chunks, url, pwd, total_size))
    }
}

// ========================================================
// WEBDAV TRANSLATION LAYER (INFUSE / JELLYFIN)
// ========================================================
fn get_resolved_children(
    tree: &crate::openwrt_heriheri::VfsTree,
    pid: u64,
) -> Vec<(crate::openwrt_heriheri::VfsNode, String)> {
    let mut children: Vec<_> = tree.nodes.values()
        .filter(|n| n.pid == pid && !n.is_deleted && !n.is_trashed)
        .collect();
    
    // Sort deterministically by ID (oldest first) to stabilize Syncs
    children.sort_by_key(|n| n.id);

    let mut seen_names: HashMap<String, u32> = HashMap::new();
    let mut resolved = Vec::with_capacity(children.len());

    for child in children {
        let mut current_name = child.name.clone();
        let count = seen_names.entry(current_name.clone()).or_insert(0);
        if *count > 0 {
            if let Some(idx) = current_name.rfind('.') {
                let (name, ext) = current_name.split_at(idx);
                current_name = format!("{} ({}){}", name, count, ext);
            } else {
                current_name = format!("{} ({})", current_name, count);
            }
        }
        *count += 1;
        resolved.push((child.clone(), current_name));
    }

    resolved
}

async fn handle_dav_dispatch(
    method: axum::http::Method,
    uri: axum::http::Uri,
    AxumState(state): AxumState<Arc<AppState>>,
    headers: HeaderMap,
) -> Response {
    println!("\n[WEBDAV] ==========================================");
    println!("[WEBDAV] INCOMING REQUEST: {} {}", method, uri);
    println!(
        "[WEBDAV] Auth Header Present: {}",
        headers.contains_key(header::AUTHORIZATION)
    );

    let expected_auth = {
        let config_arc = get_config();
        let config = config_arc.lock().await;
        let auth_raw = format!("{}:{}", config.username, config.password);
        use base64::Engine;
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(auth_raw)
        )
    };

    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if auth != expected_auth {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("WWW-Authenticate", "Basic realm=\"HeriHeri WebDAV\"")
            .body(axum::body::Body::empty())
            .unwrap();
    }

    let p = uri.path().strip_prefix("/dav").unwrap_or("");
    let depth = headers
        .get("Depth")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("1");

    let vfs_guard = state.vfs.lock().await;
    let tree = match vfs_guard.as_ref() {
        Some(t) => t,
        None => return (StatusCode::INTERNAL_SERVER_ERROR, "VFS Offline").into_response(),
    };

    let mut current_id = 0;
    let mut is_dir = true;
    let mut current_node = tree.nodes.get(&0).cloned();
    let mut resolved_node_name = "Root".to_string();

    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    println!("[WEBDAV] 🗺️ Target Path: '{}' | Parts: {:?}", p, parts);

    for part in parts {
        let decoded_part = decode_url(part);
        println!(
            "[WEBDAV] 🔎 Searching for segment: '{}' (Decoded: '{}') under PID: {}",
            part, decoded_part, current_id
        );

        let children = get_resolved_children(tree, current_id);
        let mut found = None;

        for (node, resolved_name) in children {
            if resolved_name == decoded_part {
                found = Some((node, resolved_name));
                break;
            }
        }

        if let Some((n, r_name)) = found {
            current_id = n.id;
            is_dir = n.node_type == crate::openwrt_heriheri::NodeType::Directory;
            resolved_node_name = r_name;
            current_node = Some(n);
        } else {
            println!(
                "[WEBDAV] 404 NOT FOUND: Could not find '{}' under PID {}",
                decoded_part, current_id
            );
            return StatusCode::NOT_FOUND.into_response();
        }
    }

    match method.as_str() {
        "OPTIONS" => Response::builder()
            .header("Allow", "OPTIONS, GET, HEAD, PROPFIND")
            .header("DAV", "1, 2")
            .body(axum::body::Body::empty())
            .unwrap(),
        "PROPFIND" => {
            let mut xml = String::from(
                "<?xml version=\"1.0\" encoding=\"utf-8\" ?>\n<D:multistatus xmlns:D=\"DAV:\">\n",
            );

            append_propfind_node(&mut xml, is_dir, current_node.as_ref(), &resolved_node_name, p);

            if depth == "1" && is_dir {
                let children = get_resolved_children(tree, current_id);
                for (child, resolved_name) in children {
                    let child_path = if p.is_empty() || p == "/" {
                        format!("/{}", resolved_name)
                    } else {
                        format!("{}/{}", p, resolved_name)
                    };
                    let child_is_dir = child.node_type == crate::openwrt_heriheri::NodeType::Directory;
                    append_propfind_node(&mut xml, child_is_dir, Some(&child), &resolved_name, &child_path);
                }
            }
            xml.push_str("</D:multistatus>");

            Response::builder()
                .status(StatusCode::MULTI_STATUS)
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(axum::body::Body::from(xml))
                .unwrap()
        }
        "GET" | "HEAD" => {
            if is_dir {
                return StatusCode::FORBIDDEN.into_response();
            }

            if method.as_str() == "HEAD" {
                let size = current_node.as_ref()
                    .map(|n| n.size.parse::<u64>().unwrap_or_else(|_| parse_size_to_bytes(&n.size)))
                    .unwrap_or(0);
                
                let ext = current_node.as_ref()
                    .and_then(|n| n.name.split('.').last())
                    .unwrap_or("")
                    .to_lowercase();
                
                let content_type = match ext.as_str() {
                    "mp4" => "video/mp4",
                    "mkv" => "video/x-matroska",
                    "webm" => "video/webm",
                    _ => "application/octet-stream",
                };

                drop(vfs_guard);
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_TYPE, content_type)
                    .header(header::CONTENT_LENGTH, size.to_string())
                    .body(axum::body::Body::empty())
                    .unwrap();
            }

            drop(vfs_guard);
            handle_stream(Path(current_id), AxumState(state.clone()), headers)
                .await
                .into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

fn append_propfind_node(
    xml: &mut String,
    is_dir: bool,
    node: Option<&crate::openwrt_heriheri::VfsNode>,
    display_name: &str,
    path: &str,
) {
    let mut raw_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };

    if is_dir && !raw_path.ends_with('/') {
        raw_path.push('/');
    } else if !is_dir && raw_path.ends_with('/') {
        raw_path.pop(); 
    }

    let encoded_path = raw_path
        .split('/')
        .map(|segment| url_encode_segment(segment))
        .collect::<Vec<String>>()
        .join("/");

    let mut href = format!("/dav{}", encoded_path).replace("//", "/");

    if is_dir && !href.ends_with('/') {
        href.push('/');
    }

    let name = node.map(|n| n.name.as_str()).unwrap_or("Root");

    xml.push_str("  <D:response>\n");
    xml.push_str(&format!("    <D:href>{}</D:href>\n", href));
    xml.push_str("    <D:propstat>\n");
    xml.push_str("      <D:prop>\n");
    xml.push_str(&format!(
        "        <D:displayname>{}</D:displayname>\n",
        quick_xml_escape(display_name)
    ));

    if is_dir {
        xml.push_str("        <D:resourcetype><D:collection/></D:resourcetype>\n");
    } else {
        let size = node.map(|n| parse_size_to_bytes(&n.size)).unwrap_or(0);
        xml.push_str("        <D:resourcetype/>\n");
        xml.push_str(&format!(
            "        <D:getcontentlength>{}</D:getcontentlength>\n",
            size
        ));
    }

    xml.push_str("        <D:getlastmodified>Tue, 23 Jun 2026 13:00:00 GMT</D:getlastmodified>\n");
    xml.push_str("      </D:prop>\n");
    xml.push_str("      <D:status>HTTP/1.1 200 OK</D:status>\n");
    xml.push_str("    </D:propstat>\n");
    xml.push_str("  </D:response>\n");
}

// --- 1. Serve the embedded index.html ---
async fn serve_index() -> impl IntoResponse {
    match WebUiAssets::get("index.html") {
        Some(file) => {
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html")
                .body(axum::body::Body::from(file.data))
                .unwrap()
        }
        None => {
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(axum::body::Body::from("WebUI not compiled. Ensure index.html is inside the webui/ folder!"))
                .unwrap()
        }
    }
}

// --- 1.5 Serve standard browser assets (like favicon.ico) gracefully ---
async fn serve_assets(Path(path): Path<String>) -> impl IntoResponse {
    match WebUiAssets::get(&path) {
        Some(file) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(axum::body::Body::from(file.data))
                .unwrap()
        }
        None => {
            // Silently return 404 for missing assets without spamming the terminal
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

// --- 2. Status API ---
async fn api_status(AxumState(state): AxumState<Arc<AppState>>) -> impl IntoResponse {
    let phone = state.current_phone.lock().await.clone();
    let logged_in = !phone.is_empty();
    
    Json(serde_json::json!({
        "logged_in": logged_in,
        "phone": phone,
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// --- 3. Login API ---
#[derive(serde::Deserialize)]
struct LoginReq { phone: String, password: String }

async fn api_login(
    AxumState(state): AxumState<Arc<AppState>>,
    Json(payload): Json<LoginReq>,
) -> impl IntoResponse {
    let mut root_info = None;
    
    // 1. Scope the Lanzou lock strictly to the login and init phase
    let login_success = {
        let mut lanzou = state.lanzou.lock().await;
        if lanzou.login(&payload.phone, &payload.password).await.is_ok() {
            if let Ok((root_id, deeper_id)) = lanzou.init_vfs_root().await {
                root_info = Some((root_id, deeper_id));
            }
            true
        } else {
            false
        }
    };

    // 2. If login failed, return immediately
    if !login_success {
        return (StatusCode::UNAUTHORIZED, "Login Failed");
    }

    // 3. Save to config and update state (Now safely OUTSIDE the Lanzou lock)
    *state.current_phone.lock().await = payload.phone.clone();
    
    {
        let config_arc = get_config();
        let mut config = config_arc.lock().await;
        config.lanzou_phone = payload.phone.clone();
        config.lanzou_pass = payload.password.clone();
        let _ = std::fs::write("heriheri_config.json", serde_json::to_string(&*config).unwrap_or_default());
    }

    // 4. Build VFS and trigger Sync Pull
    if let Some((root_id, deeper_id)) = root_info {
        let file_name = format!("heriheri_tree_{}.txt", payload.phone);
        let tree_path = std::path::PathBuf::from(format!("/tmp/{}", file_name));
        
        let tree = match crate::openwrt_heriheri::VfsTree::load_local(tree_path.clone()) {
            Ok(mut t) => {
                if t.deeperdir_lanzou_id.is_empty() {
                    t.deeperdir_lanzou_id = deeper_id;
                    let _ = t.save_local();
                }
                t
            }
            Err(_) => {
                let t = crate::openwrt_heriheri::VfsTree::new(root_id, deeper_id, tree_path);
                let _ = t.save_local();
                t
            }
        };
        *state.vfs.lock().await = Some(tree);
        
        // DEADLOCK FIXED: It is now perfectly safe to call sync_pull!
        let _ = crate::openwrt_lanzou::execute_sync_pull(&state).await; 
    }

    (StatusCode::OK, "Success")
}

// --- 4. Sync Pull API ---
async fn api_sync(AxumState(state): AxumState<Arc<AppState>>) -> impl IntoResponse {
    let _guard = state.sync_lock.lock().await;
    let _ = crate::openwrt_lanzou::execute_sync_pull(&state).await;
    (StatusCode::OK, "Sync Complete")
}

// --- 5. Logout API ---
async fn api_logout(AxumState(state): AxumState<Arc<AppState>>) -> impl IntoResponse {
    *state.current_phone.lock().await = String::new();
    
    let mut lanzou = state.lanzou.lock().await;
    lanzou.ylogin = None;
    lanzou.folder_stack = vec!["-1".to_string()];
    
    let mut vfs_guard = state.vfs.lock().await;
    *vfs_guard = None; // Wipe the local tree cache from RAM
    
    // CLEAR PERSISTENT CONFIG
    {
        let config_arc = get_config();
        let mut config = config_arc.lock().await;
        config.lanzou_phone = String::new();
        config.lanzou_pass = String::new();
        let _ = std::fs::write("heriheri_config.json", serde_json::to_string(&*config).unwrap_or_default());
    }
    
    (StatusCode::OK, "Logged out")
}

// --- 6. WebDAV Config APIs ---
async fn api_get_config() -> impl IntoResponse {
    let config_arc = get_config();
    let config = config_arc.lock().await.clone();
    Json(config)
}

async fn api_set_config(Json(payload): Json<WebdavConfig>) -> impl IntoResponse {
    let mut restart_needed = false;
    {
        let config_arc = get_config();
        let mut config = config_arc.lock().await;
        config.username = payload.username.clone();
        config.password = payload.password.clone();
        
        if config.port != payload.port {
            config.port = payload.port;
            restart_needed = true;
        }
        
        // Persist to disk so the restarted daemon remembers the new settings
        let _ = std::fs::write("heriheri_config.json", serde_json::to_string(&*config).unwrap_or_default());
    }

    if restart_needed {
        // Spawn a detached task to restart the daemon after returning the HTTP 200 OK
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            use std::os::unix::process::CommandExt;
            
            println!("\n[PROXY] Port changed! Self-restarting daemon...");
            let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("./heriheri-openwrt"));
            
            // exec() completely replaces the current process, safely unbinding the old port instantly
            let _ = std::process::Command::new(exe).exec();
            std::process::exit(1); 
        });
        return (StatusCode::OK, "Restarting");
    }
    
    (StatusCode::OK, "Config Updated")
}

// ========================================================
// CORE STREAMING PROXY
// ========================================================
async fn handle_stream(
    Path(vfs_id): Path<u64>,
    AxumState(state): AxumState<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let cache = get_cache();
    let mut cached_media = None;

    {
        let mut lock = cache.lock().await;
        if let Some(entry) = lock.get(&vfs_id) {
            if Instant::now() < entry.expires_at {
                cached_media = Some(entry.clone());
            } else {
                lock.remove(&vfs_id);
            }
        }
    }

    let media = match cached_media {
        Some(m) => m,
        None => {
            println!("[PROXY] Cache Miss. Resolving from Cloud...");
            let (chunks_str, share_url, file_pwd, total_size) =
                match resolve_lanzou_media(vfs_id, &state).await {
                    Ok(res) => res,
                    Err(e) => {
                        println!("[PROXY] Failed to resolve metadata: {}", e);
                        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                    }
                };

            let downloader = state.downloader.lock().await.clone();
            let mut urls = Vec::new();

            if chunks_str == "1" || chunks_str.is_empty() {
                let direct_url = match downloader
                    .get_lanzou_direct_link(&share_url, file_pwd.as_deref())
                    .await
                {
                    Ok(u) => u,
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                    }
                };
                urls.push(direct_url);
            } else {
                let mut all_files = match downloader
                    .get_lanzou_folder_metadata(&share_url, file_pwd.as_deref())
                    .await
                {
                    Ok(f) => f,
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                    }
                };

                let re_legacy = regex::Regex::new(r"_part(\d+)\.iso").unwrap();
                let re_covert = regex::Regex::new(r"^[0-9a-f]{32}([0-9a-f]{4})\.zip$").unwrap();
                
                all_files.sort_by(|a, b| {
                    let na = a.get("name_all").and_then(|n| n.as_str()).unwrap_or_else(|| a.get("name").and_then(|n| n.as_str()).unwrap_or(""));
                    let nb = b.get("name_all").and_then(|n| n.as_str()).unwrap_or_else(|| b.get("name").and_then(|n| n.as_str()).unwrap_or(""));

                    let mut get_idx = |name: &str| -> u32 {
                        if let Some(idx) = decrypt_chunk_filename(name) { return idx; }
                        if let Some(caps) = re_covert.captures(name) { return u32::from_str_radix(&caps[1], 16).unwrap_or(0); }
                        if let Some(caps) = re_legacy.captures(name) { return caps[1].parse::<u32>().unwrap_or(0); }
                        0
                    };
                    get_idx(na).cmp(&get_idx(nb))
                });

                let parsed_share_url = reqwest::Url::parse(&share_url).unwrap();
                let base_file_url = format!("{}://{}", parsed_share_url.scheme(), parsed_share_url.host_str().unwrap());

                for file in all_files {
                    let id = file.get("id").and_then(|u| u.as_str()).unwrap_or("");
                    if id.is_empty() { return (StatusCode::INTERNAL_SERVER_ERROR, "Chunk ID missing").into_response(); }
                    urls.push(format!("{}/{}", base_file_url, id)); 
                }
            }

            let new_entry = CachedMedia {
                chunks_str,
                total_size,
                urls,
                expires_at: Instant::now() + Duration::from_secs(300),
            };

            cache.lock().await.insert(vfs_id, new_entry.clone());
            println!("[PROXY] Cloud Links Cached Successfully!");
            new_entry
        }
    };

    let total_size = media.total_size;
    let chunks_str_clone = media.chunks_str.clone();

    let calculated_clamp = (total_size as f64 * 0.005) as usize;
    let prefetch_clamp = calculated_clamp.clamp(2 * 1024 * 1024, 10 * 1024 * 1024);

    let mut start_bytes = 0;
    let mut end_bytes = total_size - 1;
    let mut is_partial = false;

    if let Some(range_header) = headers.get(header::RANGE).and_then(|r| r.to_str().ok()) {
        if let Some(ranges) = range_header.strip_prefix("bytes=") {
            let parts: Vec<&str> = ranges.split('-').collect();
            if !parts.is_empty() {
                if let Ok(s) = parts[0].parse::<usize>() {
                    start_bytes = s;
                    is_partial = true;
                }
                if parts.len() > 1 && !parts[1].is_empty() {
                    if let Ok(e) = parts[1].parse::<usize>() {
                        end_bytes = e.min(total_size - 1);
                    }
                }
            }
        }
    }

    if start_bytes >= total_size {
        return (
            StatusCode::RANGE_NOT_SATISFIABLE,
            [(header::CONTENT_RANGE, format!("bytes */{}", total_size))],
            "Range Out of Bounds",
        )
            .into_response();
    }

    let chunk_length = end_bytes - start_bytes + 1;
    let req_client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .unwrap();

    let state_clone = Arc::clone(&state);

    let body_stream: BoxStream<'static, Result<axum::body::Bytes, std::io::Error>> =
        if chunks_str_clone == "1" || chunks_str_clone.is_empty() {
            let mut active_url = media.urls[0].clone();

            let stream = async_stream::try_stream! {
                let mut current_global_ptr = start_bytes;

                while current_global_ptr <= end_bytes {
                    let clamped_end = std::cmp::min(end_bytes, current_global_ptr + prefetch_clamp - 1);
                    let mut retry = 0;

                    loop {
                        let resp = req_client.get(&active_url)
                            .header("Range", format!("bytes={}-{}", current_global_ptr, clamped_end))
                            .send()
                            .await
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                        if resp.status().is_success() {
                            let full_chunk = resp.bytes().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                            current_global_ptr += full_chunk.len() as usize;
                            yield full_chunk;
                            break;
                        } else {
                            if retry > 0 { Err(std::io::Error::new(std::io::ErrorKind::Other, "Persistent CDN Error"))?; }
                            retry += 1;

                            println!("[PROXY] CDN Rejected Link (HTTP {}). Auto-refreshing cache...", resp.status());
                            get_cache().lock().await.remove(&vfs_id);

                            let (_, new_share_url, new_file_pwd, _) = resolve_lanzou_media(vfs_id, &state_clone)
                                .await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                            let downloader = state_clone.downloader.lock().await.clone();
                            active_url = downloader.get_lanzou_direct_link(&new_share_url, new_file_pwd.as_deref())
                                .await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                            let new_entry = CachedMedia {
                                chunks_str: chunks_str_clone.clone(),
                                total_size,
                                urls: vec![active_url.clone()],
                                expires_at: Instant::now() + Duration::from_secs(300),
                            };
                            get_cache().lock().await.insert(vfs_id, new_entry);
                            println!("[PROXY] Cache Refreshed. Resuming stream.");
                        }
                    }
                }
            };
            Box::pin(stream)
        } else {
            let active_urls = media.urls.clone();
            let downloader_stream = state_clone.downloader.lock().await.clone();

            let stream = async_stream::try_stream! {
                let mut remaining_to_send = chunk_length;
                let mut current_global_ptr = start_bytes;

                let mut pre_resolve_tasks: HashMap<usize, tokio::task::JoinHandle<Result<String, String>>> = HashMap::new();

                while remaining_to_send > 0 {
                    let chunk_idx = current_global_ptr / CHUNK_SIZE;
                    if chunk_idx >= active_urls.len() { break; }

                    let chunk_local_start = current_global_ptr % CHUNK_SIZE;
                    let chunk_local_end = std::cmp::min(CHUNK_SIZE - 1, chunk_local_start + remaining_to_send - 1);
                    let clamped_end = std::cmp::min(chunk_local_end, chunk_local_start + prefetch_clamp - 1);
                    
                    let chunk_share_url = &active_urls[chunk_idx];
                    let mut retry = 0;
                    let mut direct_url = String::new();

                    loop {
                        if direct_url.is_empty() {
                            if let Some(task) = pre_resolve_tasks.remove(&chunk_idx) {
                                direct_url = task.await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                            } else {
                                direct_url = downloader_stream.get_lanzou_direct_link(chunk_share_url, None).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                            }
                        }

                        for offset in 1..=2 {
                            let target_idx = chunk_idx + offset;
                            if target_idx < active_urls.len() && !pre_resolve_tasks.contains_key(&target_idx) {
                                let dl_clone = downloader_stream.clone();
                                let next_share_url = active_urls[target_idx].clone();
                                pre_resolve_tasks.insert(target_idx, tokio::spawn(async move {
                                    dl_clone.get_lanzou_direct_link(&next_share_url, None).await
                                }));
                            }
                        }

                        let resp_res = req_client.get(&direct_url)
                            .header("Range", format!("bytes={}-{}", chunk_local_start, clamped_end))
                            .send()
                            .await;

                        match resp_res {
                            Ok(resp) => {
                                let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
                                
                                if resp.status().is_success() && !ctype.contains("text/html") {
                                    let full_chunk = resp.bytes().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                                    current_global_ptr += full_chunk.len() as usize;
                                    remaining_to_send -= full_chunk.len() as usize;
                                    yield full_chunk;
                                    break; 
                                } else {
                                    println!("[PROXY] CDN Rejected Link (HTTP {} | {}). Retrying JIT...", resp.status(), ctype);
                                    if retry > 3 { Err(std::io::Error::new(std::io::ErrorKind::Other, "Persistent CDN Error"))?; }
                                    retry += 1;
                                    
                                    direct_url = String::new();
                                    for (_, task) in pre_resolve_tasks.drain() { task.abort(); }
                                }
                            },
                            Err(e) => {
                                println!("[PROXY] Network Error: {}", e);
                                if retry > 3 { Err(std::io::Error::new(std::io::ErrorKind::Other, "Network Error"))?; }
                                retry += 1;
                            }
                        }
                    }
                }
            };
            Box::pin(stream)
        };

    let ext = {
        let vfs_guard = state.vfs.lock().await;
        vfs_guard
            .as_ref()
            .and_then(|t| t.nodes.get(&vfs_id))
            .and_then(|n| n.name.split('.').last())
            .unwrap_or("")
            .to_lowercase()
    };

    let content_type = match ext.as_str() {
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "c" | "cpp" | "rs" | "py" | "js" | "ts" | "md" | "log" => {
            "text/plain; charset=utf-8"
        }
        "json" => "application/json",
        "xls" => "application/vnd.ms-excel",
        "doc" => "application/msword",
        "ppt" => "application/vnd.ms-powerpoint",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    };

    let mut response_builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

    if is_partial {
        response_builder = response_builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start_bytes, end_bytes, total_size),
            )
            .header(header::CONTENT_LENGTH, chunk_length.to_string());
    } else {
        response_builder = response_builder
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, total_size.to_string());
    }

    response_builder
        .body(axum::body::Body::from_stream(body_stream))
        .unwrap()
}