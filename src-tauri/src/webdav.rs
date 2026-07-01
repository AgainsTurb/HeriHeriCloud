use axum::http::{header, StatusCode};
use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::lanzou::{AppState, SharePayload};

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
}

static WEBDAV_CONFIG: std::sync::OnceLock<Arc<tokio::sync::Mutex<WebdavConfig>>> =
    std::sync::OnceLock::new();

pub fn get_config() -> Arc<tokio::sync::Mutex<WebdavConfig>> {
    WEBDAV_CONFIG
        .get_or_init(|| {
            Arc::new(tokio::sync::Mutex::new(WebdavConfig {
                port: 8888,
                username: "admin".to_string(),
                password: "admin".to_string(),
            }))
        })
        .clone()
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

// Minimal UTF-8 URL Decoder to prevent Cargo.toml dependency changes
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

    let app = Router::new()
        // --- NATIVE PLAYER MOUNT ---
        .route("/stream/:vfs_id", get(handle_stream))
        // --- INFUSE / JELLYFIN WEBDAV MOUNT ---
        .route("/dav", axum::routing::any(handle_dav_dispatch))
        .route("/dav/", axum::routing::any(handle_dav_dispatch))
        .route("/dav/*path", axum::routing::any(handle_dav_dispatch))
        // --- CATCH-ALL INTERCEPTOR ---
        .fallback(fallback_logger)
        .with_state(shared_state);

    let config_arc = get_config();
    let config = config_arc.lock().await.clone();
    let port = config.port;

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .unwrap();
    println!(
        "[PROXY] Local Video Streaming Proxy listening on port {}",
        port
    );
    println!(
        "[PROXY] WebDAV Mount available at http://127.0.0.1:{}/dav (User: {}, Pass: {})",
        port, config.username, config.password
    );
    axum::serve(listener, app).await.unwrap();
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
        let json_str = crate::lanzou::decrypt_payload(&encoded)
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
        let is_folder = node.node_type == crate::heriheri::NodeType::Directory
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

    // 1. WebDAV Basic Auth check
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

    // 2. Translate String Path to VFS ID
    let mut current_id = 0;
    let mut is_dir = true;
    let mut current_node = tree.nodes.get(&0).cloned();

    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    println!("[WEBDAV] 🗺️ Target Path: '{}' | Parts: {:?}", p, parts);

    for part in parts {
        let decoded_part = decode_url(part);
        println!(
            "[WEBDAV] 🔎 Searching for segment: '{}' (Decoded: '{}') under PID: {}",
            part, decoded_part, current_id
        );

        let mut found = None;
        for node in tree.nodes.values() {
            // Check if node belongs to current folder. (Assumes 'pid' is your parent ID property)
            if node.pid == current_id
                && node.name == decoded_part
                && !node.is_deleted
                && !node.is_trashed
            {
                found = Some(node.clone());
                break;
            }
        }
        if let Some(n) = found {
            current_id = n.id;
            is_dir = n.node_type == crate::heriheri::NodeType::Directory;
            println!("[WEBDAV] Found node: {} (ID: {})", n.name, n.id);
            current_node = Some(n);
        } else {
            println!(
                "[WEBDAV] 404 NOT FOUND: Could not find '{}' under PID {}",
                decoded_part, current_id
            );
            return StatusCode::NOT_FOUND.into_response();
        }
    }

    // 3. Route to WebDAV Standard Handlers
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

            // Render Target Node
            append_propfind_node(&mut xml, is_dir, current_node.as_ref(), p);

            // Render Children if directory requested
            if depth == "1" && is_dir {
                for child in tree.nodes.values() {
                    if child.pid == current_id && !child.is_deleted && !child.is_trashed {
                        let safe_name = quick_xml_escape(&child.name);
                        let child_path = if p.is_empty() || p == "/" {
                            format!("/{}", safe_name)
                        } else {
                            format!("{}/{}", p, safe_name)
                        };
                        let child_is_dir = child.node_type == crate::heriheri::NodeType::Directory;
                        append_propfind_node(&mut xml, child_is_dir, Some(child), &child_path);
                    }
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

            // Critical: Drop the VFS Lock before handing off to the stream engine!
            drop(vfs_guard);

            // Hand off smoothly to the core high-performance streamer
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
    node: Option<&crate::heriheri::VfsNode>,
    path: &str,
) {
    // 1. Format the raw path to ensure it starts with a slash
    let mut raw_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };

    // 2. Trailing Slash Mandate: Directories MUST end with a slash in WebDAV
    if is_dir && !raw_path.ends_with('/') {
        raw_path.push('/');
    } else if !is_dir && raw_path.ends_with('/') {
        raw_path.pop(); // Strip trailing slash from files just in case
    }

    // 3. URL-Encode the path segments (don't encode the slashes!)
    let encoded_path = raw_path
        .split('/')
        .map(|segment| url_encode_segment(segment))
        .collect::<Vec<String>>()
        .join("/");

    // Deduplicate any accidental double slashes
    let mut href = format!("/dav{}", encoded_path).replace("//", "/");

    // Final sanity check for Apple's strict parser: Folders MUST end with a slash.
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
        quick_xml_escape(name)
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

    // Note: The date format is hardcoded here. If Infuse complains about syncing,
    // we may need to pull the real Last-Modified date from the node later.
    xml.push_str("        <D:getlastmodified>Tue, 23 Jun 2026 13:00:00 GMT</D:getlastmodified>\n");
    xml.push_str("      </D:prop>\n");
    xml.push_str("      <D:status>HTTP/1.1 200 OK</D:status>\n");
    xml.push_str("    </D:propstat>\n");
    xml.push_str("  </D:response>\n");
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

    // --- 1. Check Memory Cache ---
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

    // --- 2. Resolve & Cache if Missing ---
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
                    .get_lanzou_folder_links(&share_url, file_pwd.as_deref(), 5)
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
                    let na = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let nb = b.get("name").and_then(|n| n.as_str()).unwrap_or("");

                    let get_idx = |name: &str| -> u32 {
                        // Check legacy format
                        if let Some(caps) = re_legacy.captures(name) {
                            return caps[1].parse::<u32>().unwrap_or(0);
                        }
                        // Check new covert hex format
                        if let Some(caps) = re_covert.captures(name) {
                            return u32::from_str_radix(&caps[1], 16).unwrap_or(0);
                        }
                        0
                    };

                    get_idx(na).cmp(&get_idx(nb))
                });

                for file in all_files {
                    let u = file
                        .get("direct_url")
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string();
                    if u.is_empty() || u == "null" {
                        return (StatusCode::INTERNAL_SERVER_ERROR, "Chunk URL unresolved")
                            .into_response();
                    }
                    urls.push(u);
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

    // --- Dynamic Prefetch Clamp Algorithm ---
    let calculated_clamp = (total_size as f64 * 0.005) as usize;
    let prefetch_clamp = calculated_clamp.clamp(2 * 1024 * 1024, 10 * 1024 * 1024);

    // --- 3. Parse Byte Range ---
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

    // --- 4. Pipeline Engine with Eager Buffering, Clamping, and Auto-Refresh ---
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
            let mut active_urls = media.urls.clone();

            let stream = async_stream::try_stream! {
                let mut remaining_to_send = chunk_length;
                let mut current_global_ptr = start_bytes;

                while remaining_to_send > 0 {
                    let chunk_idx = current_global_ptr / CHUNK_SIZE;
                    if chunk_idx >= active_urls.len() { break; }

                    let chunk_local_start = current_global_ptr % CHUNK_SIZE;
                    let chunk_local_end = std::cmp::min(CHUNK_SIZE - 1, chunk_local_start + remaining_to_send - 1);

                    let clamped_end = std::cmp::min(chunk_local_end, chunk_local_start + prefetch_clamp - 1);
                    let mut retry = 0;

                    loop {
                        let direct_url = &active_urls[chunk_idx];
                        let resp = req_client.get(direct_url)
                            .header("Range", format!("bytes={}-{}", chunk_local_start, clamped_end))
                            .send()
                            .await
                            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                        if resp.status().is_success() {
                            let full_chunk = resp.bytes().await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                            current_global_ptr += full_chunk.len() as usize;
                            remaining_to_send -= full_chunk.len() as usize;
                            yield full_chunk;
                            break;
                        } else {
                            if retry > 0 { Err(std::io::Error::new(std::io::ErrorKind::Other, "Persistent CDN Error"))?; }
                            retry += 1;

                            println!("[PROXY] CDN Rejected Chunk Link (HTTP {}). Auto-refreshing cache...", resp.status());
                            get_cache().lock().await.remove(&vfs_id);

                            let (_, new_share_url, new_file_pwd, _) = resolve_lanzou_media(vfs_id, &state_clone)
                                .await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                            let downloader = state_clone.downloader.lock().await.clone();
                            let mut all_files = downloader.get_lanzou_folder_links(&new_share_url, new_file_pwd.as_deref(), 5)
                                .await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

                            let re_legacy = regex::Regex::new(r"_part(\d+)\.iso").unwrap();
                            let re_covert = regex::Regex::new(r"^[0-9a-f]{32}([0-9a-f]{4})\.zip$").unwrap();

                            all_files.sort_by(|a, b| {
                                let na = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                let nb = b.get("name").and_then(|n| n.as_str()).unwrap_or("");

                                let get_idx = |name: &str| -> u32 {
                                    // Check legacy format
                                    if let Some(caps) = re_legacy.captures(name) {
                                        return caps[1].parse::<u32>().unwrap_or(0);
                                    }
                                    // Check new covert hex format
                                    if let Some(caps) = re_covert.captures(name) {
                                        return u32::from_str_radix(&caps[1], 16).unwrap_or(0);
                                    }
                                    0
                                };

                                get_idx(na).cmp(&get_idx(nb))
                            });

                            let mut new_urls = Vec::new();
                            for file in all_files {
                                new_urls.push(file.get("direct_url").and_then(|u| u.as_str()).unwrap_or("").to_string());
                            }
                            active_urls = new_urls.clone();

                            let new_entry = CachedMedia {
                                chunks_str: chunks_str_clone.clone(),
                                total_size,
                                urls: new_urls,
                                expires_at: Instant::now() + Duration::from_secs(300),
                            };
                            get_cache().lock().await.insert(vfs_id, new_entry);
                            println!("[PROXY] Cache Refreshed. Resuming stream.");
                        }
                    }
                }
            };
            Box::pin(stream)
        };

    // --- 5. Return HTTP Headers ---
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

// ========================================================
// TAURI FRONTEND COMMANDS
// ========================================================
#[tauri::command]
pub async fn get_webdav_config() -> Result<WebdavConfig, String> {
    let config_arc = get_config();
    let config = config_arc.lock().await.clone();
    Ok(config)
}

#[tauri::command]
pub async fn set_webdav_config(
    port: u16,
    username: String,
    password: String,
) -> Result<(), String> {
    let config_arc = get_config();
    let mut config = config_arc.lock().await;
    config.port = port;
    config.username = username;
    config.password = password;
    Ok(())
}

#[tauri::command]
pub async fn boot_webdav_server(
    port: u16,
    username: String,
    password: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // 1. Prevent the server from booting twice
    if SERVER_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    // 2. Overwrite the RAM config with the frontend's saved data
    {
        let config_arc = get_config();
        let mut config = config_arc.lock().await;
        config.port = port;
        config.username = username;
        config.password = password;
    }

    // 3. Clone the app state and ignite the Axum server in the background
    let app_state = state.inner().clone();
    tokio::spawn(async move {
        run_server(app_state).await;
    });

    Ok(())
}

#[tauri::command]
pub fn get_local_ip() -> String {
    // Zero-dependency trick to find the active local network IP
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "192.168.x.x".to_string() // Safe fallback
}
