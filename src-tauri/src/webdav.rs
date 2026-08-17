use axum::http::{header, StatusCode};
use axum::{
    extract::{Path, State as AxumState},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use reqwest::Client;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};

use crate::lanzou::{AppState, SharePayload};

const CHUNK_SIZE: u64 = 100 * 1024 * 1024;
const DIRECT_LINK_REFRESH_AFTER: Duration = Duration::from_secs(150);
const DIRECT_LINK_TTL: Duration = Duration::from_secs(240);
const DIRECT_LINK_WARM_CONCURRENCY: usize = 4;
const DIRECT_LINK_REFRESH_POLL: Duration = Duration::from_secs(20);
const MEDIA_ACTIVE_WINDOW: Duration = Duration::from_secs(600);
const MEDIA_ACTIVITY_TOUCH_INTERVAL: Duration = Duration::from_secs(20);
const CLOUD_RESOLVE_TIMEOUT: Duration = Duration::from_secs(45);
const UPSTREAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const UPSTREAM_PACKET_TIMEOUT: Duration = Duration::from_secs(8);
const MEDIA_SOURCE_REPLACED: &str = "Media source was replaced";

static SERVER_PORT: std::sync::OnceLock<Arc<Mutex<Option<u16>>>> = std::sync::OnceLock::new();

fn get_server_port() -> Arc<Mutex<Option<u16>>> {
    SERVER_PORT
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone()
}

// ========================================================
// MEMORY CACHE ENGINE
// ========================================================
#[derive(Clone)]
struct CachedMedia {
    source_key: String,
    chunks_str: String,
    total_size: u64,
    urls: Vec<String>,
    chunk_password: Option<String>,
    direct_urls: Vec<Option<CachedDirectUrl>>,
    last_accessed_at: Instant,
}

#[derive(Clone)]
struct CachedDirectUrl {
    url: String,
    refresh_at: Instant,
    expires_at: Instant,
}

impl CachedDirectUrl {
    fn new(url: String) -> Self {
        let now = Instant::now();
        Self {
            url,
            refresh_at: now + DIRECT_LINK_REFRESH_AFTER,
            expires_at: now + DIRECT_LINK_TTL,
        }
    }

    fn is_usable_at(&self, now: Instant) -> bool {
        now < self.expires_at
    }

    fn needs_refresh_at(&self, now: Instant) -> bool {
        now >= self.refresh_at
    }
}

static URL_CACHE: std::sync::OnceLock<Arc<Mutex<HashMap<u64, CachedMedia>>>> =
    std::sync::OnceLock::new();
type MediaResolveLocks = Arc<Mutex<HashMap<(u64, String), Arc<Mutex<()>>>>>;
static MEDIA_RESOLVE_LOCKS: std::sync::OnceLock<MediaResolveLocks> = std::sync::OnceLock::new();
type DirectResolveLocks = Arc<Mutex<HashMap<(u64, String, usize), Arc<Mutex<()>>>>>;
static DIRECT_RESOLVE_LOCKS: std::sync::OnceLock<DirectResolveLocks> = std::sync::OnceLock::new();
static DIRECT_LINK_WARM_SEMAPHORE: std::sync::OnceLock<Arc<Semaphore>> = std::sync::OnceLock::new();
static WARMING_DIRECT_LINKS: std::sync::OnceLock<Arc<Mutex<HashSet<(u64, String, usize)>>>> =
    std::sync::OnceLock::new();
static ACTIVE_REFRESH_SUPERVISORS: std::sync::OnceLock<Arc<Mutex<HashSet<(u64, String)>>>> =
    std::sync::OnceLock::new();
static STREAM_CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
static LEGACY_CHUNK_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
static COVERT_CHUNK_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

fn get_cache() -> Arc<Mutex<HashMap<u64, CachedMedia>>> {
    URL_CACHE
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

fn chunk_index_from_name(name: &str) -> Option<u32> {
    if let Some(index) = decrypt_chunk_filename(name) {
        return Some(index);
    }
    let covert = COVERT_CHUNK_RE
        .get_or_init(|| regex::Regex::new(r"^[0-9a-f]{32}([0-9a-f]{4})\.zip$").unwrap());
    if let Some(captures) = covert.captures(name) {
        return u32::from_str_radix(&captures[1], 16).ok();
    }
    let legacy = LEGACY_CHUNK_RE.get_or_init(|| regex::Regex::new(r"_part(\d+)\.iso$").unwrap());
    legacy
        .captures(name)
        .and_then(|captures| captures[1].parse::<u32>().ok())
}

fn chunk_file_name(file: &serde_json::Value) -> &str {
    file.get("name_all")
        .and_then(|name| name.as_str())
        .or_else(|| file.get("name").and_then(|name| name.as_str()))
        .unwrap_or("")
}

/// Orders a Lanzou folder without weakening the normal encrypted-index validation. The filename
/// key is local configuration, so another computer can legitimately be unable to decrypt names
/// created by the first computer. Lanzou returns folder entries newest-first while HeriHeriCloud
/// uploads chunks sequentially; the shorter tail chunk additionally tells us the orientation when
/// the file size is not an exact multiple of CHUNK_SIZE.
///
/// Returns true when the listing-order compatibility path was required.
fn order_cloud_chunks(
    files: &mut [serde_json::Value],
    expected_chunks: usize,
    total_size: u64,
) -> Result<bool, String> {
    if files.len() != expected_chunks {
        return Err(format!(
            "cloud folder contains {} chunks; expected {expected_chunks}",
            files.len()
        ));
    }

    let parsed_indexes: Vec<Option<u32>> = files
        .iter()
        .map(|file| chunk_index_from_name(chunk_file_name(file)))
        .collect();
    let recognized = parsed_indexes
        .iter()
        .filter(|index| index.is_some())
        .count();

    if recognized == expected_chunks {
        files.sort_by_key(|file| chunk_index_from_name(chunk_file_name(file)));
        let ordered_indexes: Vec<u32> = files
            .iter()
            .filter_map(|file| chunk_index_from_name(chunk_file_name(file)))
            .collect();
        let expected_indexes: Vec<u32> = (1..=expected_chunks as u32).collect();
        if ordered_indexes != expected_indexes {
            return Err("cloud folder has missing or duplicate chunk indexes".to_string());
        }
        return Ok(false);
    }

    if recognized != 0 {
        return Err("cloud folder mixes recognized and unrecognized chunk indexes".to_string());
    }

    // With no decryptable names, retain an already oldest-first response when its final entry is
    // the expected short tail. Otherwise use Lanzou's normal newest-first ordering.
    let tail_size = total_size.saturating_sub(
        CHUNK_SIZE
            .saturating_mul(u64::try_from(expected_chunks.saturating_sub(1)).unwrap_or(u64::MAX)),
    );
    let first_size = files
        .first()
        .and_then(|file| file.get("size"))
        .and_then(|size| size.as_str())
        .map(parse_size_to_bytes)
        .unwrap_or(0);
    let last_size = files
        .last()
        .and_then(|file| file.get("size"))
        .and_then(|size| size.as_str())
        .map(parse_size_to_bytes)
        .unwrap_or(0);
    let already_oldest_first = tail_size < CHUNK_SIZE
        && last_size > 0
        && last_size < CHUNK_SIZE
        && first_size >= CHUNK_SIZE;
    if !already_oldest_first {
        files.reverse();
    }
    Ok(true)
}

#[derive(Debug, PartialEq, Eq)]
struct ChunkWindow {
    index: usize,
    local_start: u64,
    local_end: u64,
    length: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum RecoveryAction {
    RetrySameUrl,
    RefreshLink,
    Fail,
}

#[derive(Default)]
struct StreamRecovery {
    retried_current_url: bool,
    refreshed_link: bool,
}

impl StreamRecovery {
    fn transport_failure(&mut self) -> RecoveryAction {
        if !self.retried_current_url {
            self.retried_current_url = true;
            RecoveryAction::RetrySameUrl
        } else if !self.refreshed_link {
            self.refreshed_link = true;
            self.retried_current_url = false;
            RecoveryAction::RefreshLink
        } else {
            RecoveryAction::Fail
        }
    }

    fn rejected_link(&mut self) -> RecoveryAction {
        if self.refreshed_link {
            RecoveryAction::Fail
        } else {
            self.refreshed_link = true;
            self.retried_current_url = false;
            RecoveryAction::RefreshLink
        }
    }
}

fn normalized_content_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn upstream_content_type_is_rejected(content_type: &str, expected_content_type: &str) -> bool {
    let media_type = normalized_content_type(content_type);
    let expected_media_type = normalized_content_type(expected_content_type);
    media_type != expected_media_type
        && (media_type == "text/html"
            || media_type == "application/json"
            || media_type.ends_with("+json"))
}

fn next_chunk_window(
    global_offset: u64,
    remaining: u64,
    chunk_count: usize,
) -> Option<ChunkWindow> {
    if remaining == 0 {
        return None;
    }
    let index = usize::try_from(global_offset / CHUNK_SIZE).ok()?;
    if index >= chunk_count {
        return None;
    }
    let local_start = global_offset % CHUNK_SIZE;
    let available = CHUNK_SIZE - local_start;
    let length = remaining.min(available);
    Some(ChunkWindow {
        index,
        local_start,
        local_end: local_start + length - 1,
        length,
    })
}

fn direct_link_warm_order(chunk_count: usize) -> Vec<usize> {
    if chunk_count == 0 {
        return Vec::new();
    }

    let mut order = Vec::with_capacity(chunk_count);
    let mut seen = vec![false; chunk_count];
    let push_unique = |index: usize, order: &mut Vec<usize>, seen: &mut [bool]| {
        if index < seen.len() && !seen[index] {
            seen[index] = true;
            order.push(index);
        }
    };
    push_unique(0, &mut order, &mut seen);
    push_unique(chunk_count - 1, &mut order, &mut seen);
    push_unique(1, &mut order, &mut seen);

    let mut intervals = VecDeque::from([(1usize, chunk_count - 1)]);
    while let Some((start, end)) = intervals.pop_front() {
        if end <= start + 1 {
            continue;
        }
        let midpoint = start + (end - start) / 2;
        push_unique(midpoint, &mut order, &mut seen);
        intervals.push_back((start, midpoint));
        intervals.push_back((midpoint, end));
    }
    order
}

fn next_seek_chunk_to_warm(start_bytes: u64, chunk_count: usize) -> Option<usize> {
    let current = usize::try_from(start_bytes / CHUNK_SIZE).ok()?;
    current.checked_add(1).filter(|next| *next < chunk_count)
}

async fn media_resolve_lock(vfs_id: u64, source_key: &str) -> Arc<Mutex<()>> {
    let locks = MEDIA_RESOLVE_LOCKS
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone();
    let mut locks = locks.lock().await;
    locks
        .entry((vfs_id, source_key.to_string()))
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn direct_resolve_lock(vfs_id: u64, source_key: &str, chunk_index: usize) -> Arc<Mutex<()>> {
    let locks = DIRECT_RESOLVE_LOCKS
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone();
    let mut locks = locks.lock().await;
    locks
        .entry((vfs_id, source_key.to_string(), chunk_index))
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn cached_media(vfs_id: u64, source_key: &str) -> Option<CachedMedia> {
    let cache = get_cache();
    let mut cache = cache.lock().await;
    let entry = cache
        .get_mut(&vfs_id)
        .filter(|entry| entry.source_key == source_key)?;
    entry.last_accessed_at = Instant::now();
    Some(entry.clone())
}

fn media_source_key(node: &crate::heriheri::VfsNode) -> String {
    format!(
        "{}:{}|{}:{}|{}:{}|{}:{}|{}",
        node.lanzou_id.len(),
        node.lanzou_id,
        node.chunks.len(),
        node.chunks,
        node.size.len(),
        node.size,
        node.md5.len(),
        node.md5,
        node.time,
    )
}

async fn current_media_source_key(vfs_id: u64, state: &AppState) -> Result<String, String> {
    let vfs = state.vfs.lock().await;
    let node = vfs
        .as_ref()
        .and_then(|tree| tree.nodes.get(&vfs_id))
        .ok_or_else(|| "Node not found".to_string())?;
    if node.node_type == crate::heriheri::NodeType::Directory {
        return Err("Cannot stream a directory".to_string());
    }
    Ok(media_source_key(node))
}

async fn media_source_is_current(vfs_id: u64, source_key: &str, state: &AppState) -> bool {
    current_media_source_key(vfs_id, state)
        .await
        .is_ok_and(|current| current == source_key)
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

fn get_filename_cipher() -> ChaCha20Poly1305 {
    // Attempt to load CHUNKS_NAMES_KEY; fallback to the main secret key if missing
    let secret = std::env::var("CHUNKS_NAMES_KEY")
        .unwrap_or_else(|_| env!("HERIHERI_SECRET_KEY").to_string());

    let mut key_bytes = [0u8; 32];
    let bytes = secret.as_bytes();
    let len = std::cmp::min(bytes.len(), 32);
    key_bytes[..len].copy_from_slice(&bytes[..len]);

    let key = Key::from_slice(&key_bytes);
    ChaCha20Poly1305::new(key)
}

/// Encrypts the payload into an un-analyzable 36-character string matching Lanzou constraints
pub fn encrypt_chunk_filename(md5_str: &str, chunk_index: u32) -> String {
    let cipher = get_filename_cipher();

    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    let plaintext = format!("{}{:04x}", md5_str, chunk_index);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("Crypto Fail");

    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&ciphertext);

    hex::encode(payload)
}

/// Attempts to parse and decrypt a filename. Returns Some(index) if it matches our scheme.
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

fn parse_byte_range(range_header: Option<&str>, total_size: u64) -> Result<(u64, u64, bool), ()> {
    if total_size == 0 {
        return Err(());
    }
    let Some(range_header) = range_header else {
        return Ok((0, total_size - 1, false));
    };
    let range = range_header.strip_prefix("bytes=").ok_or(())?;
    if range.contains(',') {
        return Err(()); // Multipart byte ranges are intentionally unsupported.
    }
    let (start, end) = range.split_once('-').ok_or(())?;
    let (start, end) = if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let suffix = suffix.min(total_size);
        (total_size - suffix, total_size - 1)
    } else {
        let start = start.parse::<u64>().map_err(|_| ())?;
        let end = if end.is_empty() {
            total_size - 1
        } else {
            end.parse::<u64>().map_err(|_| ())?.min(total_size - 1)
        };
        (start, end)
    };
    if start >= total_size || end < start {
        return Err(());
    }
    Ok((start, end, true))
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

pub async fn run_server(listener: tokio::net::TcpListener, state: AppState, app: tauri::AppHandle) {
    let shared_state = Arc::new(state);

    let app_router = Router::new()
        .route(
            "/stream/:vfs_id",
            get(handle_stream).head(handle_stream_head),
        )
        .route("/dav", axum::routing::any(handle_dav_dispatch))
        .route("/dav/", axum::routing::any(handle_dav_dispatch))
        .route("/dav/*path", axum::routing::any(handle_dav_dispatch))
        .route("/mcp", axum::routing::post(handle_mcp_http))
        .layer(axum::Extension(app))
        .fallback(fallback_logger)
        .with_state(shared_state);

    let port = listener
        .local_addr()
        .map(|address| address.port())
        .unwrap_or(0);
    let config = get_config().lock().await.clone();
    println!(
        "[PROXY] Local Video Streaming Proxy listening on port {}",
        port
    );
    println!(
        "[PROXY] WebDAV is listening on all interfaces: http://<LAN-IP>:{}/dav (User: {}, Pass: {})",
        port, config.username, config.password
    );
    println!(
        "[MCP] Agent HTTP Endpoint ready at http://127.0.0.1:{}/mcp",
        port
    );
    if let Err(error) = axum::serve(listener, app_router).await {
        eprintln!("[WEBDAV] Server stopped unexpectedly: {error}");
    }
    let server_port = get_server_port();
    let mut running_port = server_port.lock().await;
    if *running_port == Some(port) {
        *running_port = None;
    }
}

async fn resolve_lanzou_media(
    vfs_id: u64,
    state: &AppState,
) -> Result<(String, String, Option<String>, u64), String> {
    let vfs_guard = state.vfs.lock().await;
    let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;
    let node = tree.nodes.get(&vfs_id).cloned().ok_or("Node not found")?;

    let total_size = parse_size_to_bytes(&node.size);

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
        // Clone the HTTP/session state and release the global account mutex before doing network
        // I/O. A slow share-info request must not freeze unrelated VFS commands in the UI.
        let lanzou = state.lanzou.lock().await.clone();
        let share_info = tokio::time::timeout(
            CLOUD_RESOLVE_TIMEOUT,
            lanzou.get_share_info(node.lanzou_id.clone(), is_folder),
        )
        .await
        .map_err(|_| format!("Timed out requesting share information for VFS node {vfs_id}"))??;

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

async fn resolve_lanzou_media_bounded(
    vfs_id: u64,
    state: &AppState,
) -> Result<(String, String, Option<String>, u64), String> {
    tokio::time::timeout(CLOUD_RESOLVE_TIMEOUT, resolve_lanzou_media(vfs_id, state))
        .await
        .map_err(|_| format!("Timed out resolving cloud metadata for VFS node {vfs_id}"))?
}

async fn resolve_direct_link_bounded(
    downloader: &crate::lanzou_down::LanzouDownloader,
    share_url: &str,
    password: Option<&str>,
) -> Result<String, String> {
    tokio::time::timeout(
        CLOUD_RESOLVE_TIMEOUT,
        downloader.get_lanzou_direct_link(share_url, password),
    )
    .await
    .map_err(|_| "Timed out resolving a Lanzou direct link".to_string())?
}

async fn resolve_folder_metadata_bounded(
    downloader: &crate::lanzou_down::LanzouDownloader,
    share_url: &str,
    password: Option<&str>,
) -> Result<Vec<serde_json::Value>, String> {
    tokio::time::timeout(
        CLOUD_RESOLVE_TIMEOUT,
        downloader.get_lanzou_folder_metadata(share_url, password),
    )
    .await
    .map_err(|_| "Timed out resolving Lanzou chunk metadata".to_string())?
}

// ========================================================
// WEBDAV TRANSLATION LAYER (INFUSE / JELLYFIN)
// ========================================================
fn get_resolved_children(
    tree: &crate::heriheri::VfsTree,
    pid: u64,
) -> Vec<(crate::heriheri::VfsNode, String)> {
    let mut children: Vec<_> = tree
        .nodes
        .values()
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
            is_dir = n.node_type == crate::heriheri::NodeType::Directory;
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

            append_propfind_node(
                &mut xml,
                is_dir,
                current_node.as_ref(),
                &resolved_node_name,
                p,
            );

            if depth == "1" && is_dir {
                let children = get_resolved_children(tree, current_id);
                for (child, resolved_name) in children {
                    let child_path = if p.is_empty() || p == "/" {
                        format!("/{}", resolved_name)
                    } else {
                        format!("{}/{}", p, resolved_name)
                    };
                    let child_is_dir = child.node_type == crate::heriheri::NodeType::Directory;
                    append_propfind_node(
                        &mut xml,
                        child_is_dir,
                        Some(&child),
                        &resolved_name,
                        &child_path,
                    );
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
                let size = current_node
                    .as_ref()
                    .map(|n| {
                        n.size
                            .parse::<u64>()
                            .unwrap_or_else(|_| parse_size_to_bytes(&n.size))
                    })
                    .unwrap_or(0);

                let content_type = current_node
                    .as_ref()
                    .map(|node| content_type_for_name(&node.name))
                    .unwrap_or("application/octet-stream");

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
    display_name: &str,
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
async fn handle_stream_head(
    Path(vfs_id): Path<u64>,
    AxumState(state): AxumState<Arc<AppState>>,
) -> Response {
    let vfs = state.vfs.lock().await;
    let Some(node) = vfs.as_ref().and_then(|tree| tree.nodes.get(&vfs_id)) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if node.node_type == crate::heriheri::NodeType::Directory {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let total_size = parse_size_to_bytes(&node.size);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, total_size.to_string())
        .header(header::CONTENT_TYPE, content_type_for_name(&node.name))
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn handle_stream(
    Path(vfs_id): Path<u64>,
    AxumState(state): AxumState<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    println!(
        "[STREAM] Request for VFS {vfs_id}, range={}",
        headers
            .get(header::RANGE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("full")
    );

    let source_key = match current_media_source_key(vfs_id, &state).await {
        Ok(key) => key,
        Err(error) => return (StatusCode::NOT_FOUND, error).into_response(),
    };

    // --- 1. Resolve & cache metadata/direct links with one resolver per VFS node. ---
    let media = match cached_media(vfs_id, &source_key).await {
        Some(m) => m,
        None => {
            let resolve_lock = media_resolve_lock(vfs_id, &source_key).await;
            let _resolve_guard = resolve_lock.lock().await;
            if !media_source_is_current(vfs_id, &source_key, &state).await {
                return (
                    StatusCode::CONFLICT,
                    "Media changed while its stream metadata was being resolved; retry the request",
                )
                    .into_response();
            }
            if let Some(media) = cached_media(vfs_id, &source_key).await {
                media
            } else {
                println!("[STREAM] VFS {vfs_id}: resolving cloud metadata...");
                let (chunks_str, share_url, file_pwd, total_size) =
                    match resolve_lanzou_media_bounded(vfs_id, &state).await {
                        Ok(res) => res,
                        Err(e) => {
                            println!("[PROXY] Failed to resolve metadata: {}", e);
                            return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                        }
                    };

                let downloader = state.downloader.lock().await.clone();
                let mut urls = Vec::new();

                if chunks_str == "1" || chunks_str.is_empty() {
                    let direct_url = match resolve_direct_link_bounded(
                        &downloader,
                        &share_url,
                        file_pwd.as_deref(),
                    )
                    .await
                    {
                        Ok(u) => u,
                        Err(e) => {
                            eprintln!("[STREAM] VFS {vfs_id}: direct-link resolution failed: {e}");
                            return (StatusCode::BAD_GATEWAY, e).into_response();
                        }
                    };
                    urls.push(share_url);
                    let new_entry = CachedMedia {
                        source_key: source_key.clone(),
                        chunks_str,
                        total_size,
                        urls,
                        chunk_password: file_pwd,
                        direct_urls: vec![Some(CachedDirectUrl::new(direct_url))],
                        last_accessed_at: Instant::now(),
                    };
                    if !media_source_is_current(vfs_id, &source_key, &state).await {
                        return (
                            StatusCode::CONFLICT,
                            "Media changed while its stream metadata was being resolved; retry the request",
                        )
                            .into_response();
                    }
                    get_cache().lock().await.insert(vfs_id, new_entry.clone());
                    println!(
                        "[STREAM] VFS {vfs_id}: metadata ready ({} object(s), {} bytes)",
                        new_entry.urls.len(),
                        new_entry.total_size
                    );
                    new_entry
                } else {
                    let mut all_files = match resolve_folder_metadata_bounded(
                        &downloader,
                        &share_url,
                        file_pwd.as_deref(),
                    )
                    .await
                    {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("[STREAM] VFS {vfs_id}: chunk metadata failed: {e}");
                            return (StatusCode::BAD_GATEWAY, e).into_response();
                        }
                    };

                    let expected_chunks = match chunks_str.parse::<usize>() {
                        Ok(count) if count > 1 => count,
                        _ => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                format!("Invalid chunk count '{chunks_str}' for VFS node {vfs_id}"),
                            )
                                .into_response();
                        }
                    };
                    match order_cloud_chunks(&mut all_files, expected_chunks, total_size) {
                        Ok(true) => eprintln!(
                            "[STREAM] VFS {vfs_id}: chunk-name key did not match; using validated Lanzou upload order"
                        ),
                        Ok(false) => {}
                        Err(error) => {
                            return (
                                StatusCode::BAD_GATEWAY,
                                format!("Cloud folder for VFS node {vfs_id} {error}"),
                            )
                                .into_response();
                        }
                    }

                    let parsed_share_url = match reqwest::Url::parse(&share_url) {
                        Ok(url) => url,
                        Err(error) => {
                            return (
                                StatusCode::BAD_GATEWAY,
                                format!("Invalid cloud share URL: {error}"),
                            )
                                .into_response();
                        }
                    };
                    let Some(host) = parsed_share_url.host_str() else {
                        return (
                            StatusCode::BAD_GATEWAY,
                            "Cloud share URL has no host".to_string(),
                        )
                            .into_response();
                    };
                    let base_file_url = format!("{}://{}", parsed_share_url.scheme(), host);

                    for file in all_files {
                        let id = file.get("id").and_then(|u| u.as_str()).unwrap_or("");
                        if id.is_empty() {
                            return (StatusCode::INTERNAL_SERVER_ERROR, "Chunk ID missing")
                                .into_response();
                        }
                        urls.push(format!("{}/{}", base_file_url, id)); // Store Share URLs, not Direct URLs!
                    }
                    if urls.len() != expected_chunks {
                        return (
                            StatusCode::BAD_GATEWAY,
                            format!(
                            "Cloud folder for VFS node {vfs_id} contains {} chunks; expected {}",
                            urls.len(),
                            expected_chunks
                        ),
                        )
                            .into_response();
                    }
                    let direct_urls = vec![None; urls.len()];
                    let new_entry = CachedMedia {
                        source_key: source_key.clone(),
                        chunks_str,
                        total_size,
                        urls,
                        chunk_password: file_pwd,
                        direct_urls,
                        last_accessed_at: Instant::now(),
                    };

                    if !media_source_is_current(vfs_id, &source_key, &state).await {
                        return (
                            StatusCode::CONFLICT,
                            "Media changed while its stream metadata was being resolved; retry the request",
                        )
                            .into_response();
                    }
                    get_cache().lock().await.insert(vfs_id, new_entry.clone());
                    println!(
                        "[STREAM] VFS {vfs_id}: metadata ready ({} object(s), {} bytes)",
                        new_entry.urls.len(),
                        new_entry.total_size
                    );
                    new_entry
                }
            }
        }
    };

    warm_direct_links(
        vfs_id,
        source_key.clone(),
        state.downloader.lock().await.clone(),
        media.urls.clone(),
        media.chunk_password.clone(),
    )
    .await;
    ensure_direct_link_refresh_supervisor(
        vfs_id,
        source_key.clone(),
        state.downloader.lock().await.clone(),
        media.urls.clone(),
        media.chunk_password.clone(),
    )
    .await;

    let total_size = media.total_size;
    let chunks_str_clone = media.chunks_str.clone();
    let media_name = {
        let vfs_guard = state.vfs.lock().await;
        vfs_guard
            .as_ref()
            .and_then(|tree| tree.nodes.get(&vfs_id))
            .map(|node| node.name.clone())
            .unwrap_or_default()
    };
    let response_content_type = content_type_for_name(&media_name);

    if total_size == 0 {
        return (StatusCode::NO_CONTENT, HeaderMap::new(), "").into_response();
    }

    // --- 3. Parse Byte Range ---
    let requested_range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let (start_bytes, end_bytes, is_partial) = match parse_byte_range(requested_range, total_size) {
        Ok(range) => range,
        Err(()) => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{}", total_size))],
                "Range Out of Bounds",
            )
                .into_response();
        }
    };

    let chunk_length = end_bytes - start_bytes + 1;
    let req_client = get_stream_client();

    if let Some(next_chunk) = next_seek_chunk_to_warm(start_bytes, media.urls.len()) {
        warm_seek_neighbor_link(
            vfs_id,
            source_key.clone(),
            state.downloader.lock().await.clone(),
            media.urls[next_chunk].clone(),
            media.chunk_password.clone(),
            next_chunk,
        );
    }

    let state_clone = Arc::clone(&state);

    // --- 4. Pipeline Engine with Eager Buffering, Clamping, and Auto-Refresh ---
    let body_stream: BoxStream<'static, Result<axum::body::Bytes, std::io::Error>> =
        if chunks_str_clone == "1" || chunks_str_clone.is_empty() {
            let share_url = media.urls[0].clone();
            let file_password = media.chunk_password.clone();
            let downloader_stream = state_clone.downloader.lock().await.clone();
            let stream_source_key = source_key.clone();
            let mut active_url = cached_direct_url(vfs_id, &stream_source_key, 0)
                .await
                .unwrap_or_default();

            let stream = async_stream::try_stream! {
                let mut current_global_ptr = start_bytes;
                let mut last_activity_touch = Instant::now();

                while current_global_ptr <= end_bytes {
                    let mut recovery = StreamRecovery::default();

                    loop {
                        if active_url.is_empty() {
                            active_url = resolve_cached_chunk_link(
                                vfs_id,
                                &stream_source_key,
                                0,
                                &downloader_stream,
                                &share_url,
                                file_password.as_deref(),
                            )
                            .await
                            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
                        }
                        let resp_result = tokio::time::timeout(
                            UPSTREAM_REQUEST_TIMEOUT,
                            req_client.get(&active_url)
                                .header("Range", format!("bytes={}-{}", current_global_ptr, end_bytes))
                                .send(),
                        )
                        .await;
                        let resp = match resp_result {
                            Ok(Ok(resp)) => resp,
                            Ok(Err(error)) => {
                                println!("[PROXY] Upstream request failed before headers: {error}");
                                match recovery.transport_failure() {
                                    RecoveryAction::RetrySameUrl => continue,
                                    RecoveryAction::RefreshLink => {
                                        invalidate_direct_url(
                                            vfs_id,
                                            &stream_source_key,
                                            0,
                                            &active_url,
                                        )
                                        .await;
                                        active_url.clear();
                                        continue;
                                    }
                                    RecoveryAction::Fail => Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        "Persistent upstream request error",
                                    ))?,
                                }
                            }
                            Err(_) => {
                                println!("[PROXY] Upstream request timed out before headers");
                                match recovery.transport_failure() {
                                    RecoveryAction::RetrySameUrl => continue,
                                    RecoveryAction::RefreshLink => {
                                        invalidate_direct_url(
                                            vfs_id,
                                            &stream_source_key,
                                            0,
                                            &active_url,
                                        )
                                        .await;
                                        active_url.clear();
                                        continue;
                                    }
                                    RecoveryAction::Fail => Err(std::io::Error::new(
                                        std::io::ErrorKind::TimedOut,
                                        "Persistent upstream header timeout",
                                    ))?,
                                }
                            }
                        };

                        let content_type = resp
                            .headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .and_then(|value| value.to_str().ok())
                            .unwrap_or("");
                        if resp.status().is_success()
                            && (current_global_ptr == 0 || resp.status().as_u16() == 206)
                            && !upstream_content_type_is_rejected(
                                content_type,
                                response_content_type,
                            )
                        {
                            let mut bytes = resp.bytes_stream();
                            let mut delivered = 0u64;
                            let requested = end_bytes - current_global_ptr + 1;
                            let mut interrupted_before_data = false;
                            loop {
                                let packet = match tokio::time::timeout(UPSTREAM_PACKET_TIMEOUT, bytes.next()).await {
                                    Ok(Some(packet)) => packet,
                                    Ok(None) if delivered > 0 => break,
                                    Ok(None) => {
                                        interrupted_before_data = true;
                                        break;
                                    }
                                    Err(_) if delivered > 0 => {
                                        println!("[PROXY] Upstream body stalled after {delivered} bytes. Resuming.");
                                        break;
                                    }
                                    Err(_) => {
                                        println!("[PROXY] Upstream body stalled before data. Refreshing the link.");
                                        interrupted_before_data = true;
                                        break;
                                    }
                                };
                                let packet = match packet {
                                    Ok(packet) => packet,
                                    Err(error) if delivered > 0 => {
                                        println!("[PROXY] Upstream range interrupted after {delivered} bytes: {error}. Resuming.");
                                        break;
                                    }
                                    Err(error) => {
                                        println!("[PROXY] Upstream range failed before data: {error}. Refreshing the link.");
                                        interrupted_before_data = true;
                                        break;
                                    }
                                };
                                if packet.is_empty() { continue; }
                                let take = (packet.len() as u64).min(requested - delivered) as usize;
                                if take == 0 { break; }
                                let packet = packet.slice(..take);
                                delivered += take as u64;
                                current_global_ptr += take as u64;
                                if last_activity_touch.elapsed() >= MEDIA_ACTIVITY_TOUCH_INTERVAL {
                                    touch_cached_media(vfs_id, &stream_source_key).await;
                                    last_activity_touch = Instant::now();
                                }
                                yield packet;
                                if delivered == requested { break; }
                            }
                            if interrupted_before_data {
                                match recovery.transport_failure() {
                                    RecoveryAction::RetrySameUrl => continue,
                                    RecoveryAction::RefreshLink => {
                                        invalidate_direct_url(
                                            vfs_id,
                                            &stream_source_key,
                                            0,
                                            &active_url,
                                        )
                                        .await;
                                        active_url.clear();
                                        continue;
                                    }
                                    RecoveryAction::Fail => Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        "Persistent upstream stream error",
                                    ))?,
                                }
                            }
                            if delivered == 0 {
                                Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "CDN returned an empty media range"))?;
                            }
                            break;
                        } else {
                            println!(
                                "[PROXY] CDN Rejected Link (HTTP {} | {}). Auto-refreshing cache...",
                                resp.status(),
                                content_type
                            );
                            match recovery.rejected_link() {
                                RecoveryAction::RefreshLink => {
                                    invalidate_direct_url(
                                        vfs_id,
                                        &stream_source_key,
                                        0,
                                        &active_url,
                                    )
                                    .await;
                                    active_url.clear();
                                }
                                RecoveryAction::Fail => Err(std::io::Error::new(
                                    std::io::ErrorKind::Other,
                                    "Persistent CDN rejection after link refresh",
                                ))?,
                                RecoveryAction::RetrySameUrl => unreachable!(),
                            }
                        }
                    }
                }
            };
            Box::pin(stream)
        } else {
            let active_urls = media.urls.clone();
            let chunk_password = media.chunk_password.clone();
            let stream_source_key = source_key.clone();
            let downloader_stream = state_clone.downloader.lock().await.clone();

            let stream = async_stream::try_stream! {
                let mut remaining_to_send = chunk_length;
                let mut current_global_ptr = start_bytes;
                let mut last_activity_touch = Instant::now();

                while remaining_to_send > 0 {
                    let chunk_window = next_chunk_window(
                        current_global_ptr,
                        remaining_to_send,
                        active_urls.len(),
                    ).ok_or_else(|| std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            format!("Missing cloud chunk {}", current_global_ptr / CHUNK_SIZE + 1),
                        ))?;
                    let chunk_idx = chunk_window.index;
                    let chunk_local_start = chunk_window.local_start;
                    let chunk_local_end = chunk_window.local_end;
                    let chunk_share_url = &active_urls[chunk_idx];
                    let mut recovery = StreamRecovery::default();
                    let mut direct_url = cached_direct_url(vfs_id, &stream_source_key, chunk_idx).await.unwrap_or_default();

                    loop {
                        if direct_url.is_empty() {
                            direct_url = resolve_cached_chunk_link(vfs_id, &stream_source_key, chunk_idx, &downloader_stream, chunk_share_url, chunk_password.as_deref()).await.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                        }

                        let resp_res = tokio::time::timeout(
                            UPSTREAM_REQUEST_TIMEOUT,
                            req_client.get(&direct_url)
                                .header("Range", format!("bytes={}-{}", chunk_local_start, chunk_local_end))
                                .send(),
                        ).await;

                        match resp_res {
                            Ok(Ok(resp)) => {
                                let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");

                                let honors_range = chunk_local_start == 0 || resp.status().as_u16() == 206;
                                if resp.status().is_success()
                                    && honors_range
                                    && !upstream_content_type_is_rejected(
                                        ctype,
                                        response_content_type,
                                    )
                                {
                                    let mut bytes = resp.bytes_stream();
                                    let mut delivered = 0u64;
                                    let requested = chunk_window.length;
                                    let mut interrupted_before_data = false;
                                    loop {
                                        let packet = match tokio::time::timeout(UPSTREAM_PACKET_TIMEOUT, bytes.next()).await {
                                            Ok(Some(packet)) => packet,
                                            Ok(None) if delivered > 0 => break,
                                            Ok(None) => {
                                                interrupted_before_data = true;
                                                break;
                                            }
                                            Err(_) if delivered > 0 => {
                                                println!("[PROXY] Cloud chunk {chunk_idx} stalled after {delivered} bytes. Resuming.");
                                                break;
                                            }
                                            Err(_) => {
                                                println!("[PROXY] Cloud chunk {chunk_idx} stalled before data. Refreshing the link.");
                                                interrupted_before_data = true;
                                                break;
                                            }
                                        };
                                        let packet = match packet {
                                            Ok(packet) => packet,
                                            Err(error) if delivered > 0 => {
                                                println!("[PROXY] Cloud chunk {chunk_idx} interrupted after {delivered} bytes: {error}. Resuming.");
                                                break;
                                            }
                                            Err(error) => {
                                                println!("[PROXY] Cloud chunk {chunk_idx} failed before data: {error}. Refreshing the link.");
                                                interrupted_before_data = true;
                                                break;
                                            }
                                        };
                                        if packet.is_empty() { continue; }
                                        let take =
                                            (packet.len() as u64).min(requested - delivered) as usize;
                                        if take == 0 { break; }
                                        let packet = packet.slice(..take);
                                        delivered += take as u64;
                                        current_global_ptr += take as u64;
                                        remaining_to_send =
                                            remaining_to_send.saturating_sub(take as u64);
                                        if last_activity_touch.elapsed() >= MEDIA_ACTIVITY_TOUCH_INTERVAL {
                                            touch_cached_media(vfs_id, &stream_source_key).await;
                                            last_activity_touch = Instant::now();
                                        }
                                        yield packet;
                                        if delivered == requested { break; }
                                    }
                                    if interrupted_before_data {
                                        match recovery.transport_failure() {
                                            RecoveryAction::RetrySameUrl => continue,
                                            RecoveryAction::RefreshLink => {
                                                invalidate_direct_url(
                                                    vfs_id,
                                                    &stream_source_key,
                                                    chunk_idx,
                                                    &direct_url,
                                                )
                                                .await;
                                                direct_url.clear();
                                                continue;
                                            }
                                            RecoveryAction::Fail => Err(std::io::Error::new(
                                                std::io::ErrorKind::Other,
                                                "Persistent upstream stream error",
                                            ))?,
                                        }
                                    }
                                    if delivered == 0 {
                                        Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "CDN returned an empty media chunk"))?;
                                    }
                                    break; // Success!
                                } else {
                                    println!("[PROXY] CDN Rejected Link (HTTP {} | {}). Retrying JIT...", resp.status(), ctype);
                                    match recovery.rejected_link() {
                                        RecoveryAction::RefreshLink => {
                                            invalidate_direct_url(
                                                vfs_id,
                                                &stream_source_key,
                                                chunk_idx,
                                                &direct_url,
                                            )
                                            .await;
                                            direct_url.clear();
                                        }
                                        RecoveryAction::Fail => Err(std::io::Error::new(
                                            std::io::ErrorKind::Other,
                                            "Persistent CDN rejection after link refresh",
                                        ))?,
                                        RecoveryAction::RetrySameUrl => unreachable!(),
                                    }
                                }
                            },
                            Ok(Err(e)) => {
                                println!("[PROXY] Network Error: {}", e);
                                match recovery.transport_failure() {
                                    RecoveryAction::RetrySameUrl => continue,
                                    RecoveryAction::RefreshLink => {
                                        invalidate_direct_url(
                                            vfs_id,
                                            &stream_source_key,
                                            chunk_idx,
                                            &direct_url,
                                        )
                                        .await;
                                        direct_url.clear();
                                    }
                                    RecoveryAction::Fail => Err(std::io::Error::new(
                                        std::io::ErrorKind::Other,
                                        "Persistent chunk network error",
                                    ))?,
                                }
                            },
                            Err(_) => {
                                println!("[PROXY] Cloud chunk {chunk_idx} timed out before response headers");
                                match recovery.transport_failure() {
                                    RecoveryAction::RetrySameUrl => continue,
                                    RecoveryAction::RefreshLink => {
                                        invalidate_direct_url(
                                            vfs_id,
                                            &stream_source_key,
                                            chunk_idx,
                                            &direct_url,
                                        )
                                        .await;
                                        direct_url.clear();
                                    }
                                    RecoveryAction::Fail => Err(std::io::Error::new(
                                        std::io::ErrorKind::TimedOut,
                                        "Persistent chunk header timeout",
                                    ))?,
                                }
                            }
                        }
                    }
                }
            };
            Box::pin(stream)
        };

    // --- 5. Return HTTP Headers ---
    let mut response_builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, response_content_type)
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

#[cfg(test)]
mod tests {
    use super::{
        cached_direct_url, content_type_for_name, direct_link_warm_order, direct_resolve_lock,
        direct_url_needs_refresh, get_cache, invalidate_direct_url, media_resolve_lock,
        next_chunk_window, next_seek_chunk_to_warm, order_cloud_chunks, parse_byte_range,
        parse_size_to_bytes, select_lan_ipv4, store_direct_url, touch_cached_media,
        upstream_content_type_is_rejected, CachedDirectUrl, CachedMedia, ChunkWindow,
        RecoveryAction, StreamRecovery, CHUNK_SIZE,
    };
    use serde_json::json;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, Instant};

    #[test]
    fn parses_full_open_and_suffix_ranges() {
        assert_eq!(parse_byte_range(None, 100), Ok((0, 99, false)));
        assert_eq!(
            parse_byte_range(Some("bytes=10-19"), 100),
            Ok((10, 19, true))
        );
        assert_eq!(parse_byte_range(Some("bytes=90-"), 100), Ok((90, 99, true)));
        assert_eq!(parse_byte_range(Some("bytes=-10"), 100), Ok((90, 99, true)));
        assert_eq!(parse_byte_range(Some("bytes=-200"), 100), Ok((0, 99, true)));
    }

    #[test]
    fn rejects_invalid_or_multipart_ranges() {
        assert!(parse_byte_range(Some("bytes=100-"), 100).is_err());
        assert!(parse_byte_range(Some("bytes=20-10"), 100).is_err());
        assert!(parse_byte_range(Some("bytes=0-1,3-4"), 100).is_err());
        assert!(parse_byte_range(Some("items=0-1"), 100).is_err());
        assert!(parse_byte_range(Some("bytes=-0"), 100).is_err());
        assert!(parse_byte_range(None, 0).is_err());
    }

    #[test]
    fn parses_vfs_byte_sizes() {
        assert_eq!(parse_size_to_bytes("123456"), 123456);
        assert_eq!(parse_size_to_bytes("1.5 MB"), 1_572_864);
        assert_eq!(parse_size_to_bytes("2GB"), 2_147_483_648);
    }

    #[test]
    fn orders_chunks_by_decrypted_or_legacy_indexes_when_available() {
        let mut files = vec![
            json!({"id": "second", "name_all": "asset_part2.iso", "size": CHUNK_SIZE.to_string()}),
            json!({"id": "first", "name_all": "asset_part1.iso", "size": CHUNK_SIZE.to_string()}),
        ];

        assert_eq!(order_cloud_chunks(&mut files, 2, CHUNK_SIZE * 2), Ok(false));
        assert_eq!(files[0]["id"], "first");
        assert_eq!(files[1]["id"], "second");
    }

    #[test]
    fn recovers_newest_first_chunks_when_filename_key_differs() {
        let tail_size = CHUNK_SIZE / 2;
        let mut files = vec![
            json!({"id": "third", "name_all": "opaque-third.zip", "size": tail_size.to_string()}),
            json!({"id": "second", "name_all": "opaque-second.zip", "size": CHUNK_SIZE.to_string()}),
            json!({"id": "first", "name_all": "opaque-first.zip", "size": CHUNK_SIZE.to_string()}),
        ];

        assert_eq!(
            order_cloud_chunks(&mut files, 3, CHUNK_SIZE * 2 + tail_size),
            Ok(true)
        );
        assert_eq!(files[0]["id"], "first");
        assert_eq!(files[1]["id"], "second");
        assert_eq!(files[2]["id"], "third");
    }

    #[test]
    fn retains_oldest_first_fallback_when_short_tail_already_comes_last() {
        let tail_size = CHUNK_SIZE / 2;
        let mut files = vec![
            json!({"id": "first", "name_all": "opaque-first.zip", "size": CHUNK_SIZE.to_string()}),
            json!({"id": "second", "name_all": "opaque-second.zip", "size": CHUNK_SIZE.to_string()}),
            json!({"id": "third", "name_all": "opaque-third.zip", "size": tail_size.to_string()}),
        ];

        assert_eq!(
            order_cloud_chunks(&mut files, 3, CHUNK_SIZE * 2 + tail_size),
            Ok(true)
        );
        assert_eq!(files[0]["id"], "first");
        assert_eq!(files[2]["id"], "third");
    }

    #[test]
    fn maps_seek_ranges_across_cloud_chunk_boundaries() {
        assert_eq!(
            next_chunk_window(CHUNK_SIZE - 10, 20, 2),
            Some(ChunkWindow {
                index: 0,
                local_start: CHUNK_SIZE - 10,
                local_end: CHUNK_SIZE - 1,
                length: 10,
            })
        );
        assert_eq!(
            next_chunk_window(CHUNK_SIZE, 10, 2),
            Some(ChunkWindow {
                index: 1,
                local_start: 0,
                local_end: 9,
                length: 10,
            })
        );
        assert_eq!(
            next_chunk_window(CHUNK_SIZE + 123, 50, 2),
            Some(ChunkWindow {
                index: 1,
                local_start: 123,
                local_end: 172,
                length: 50,
            })
        );
        assert_eq!(next_chunk_window(CHUNK_SIZE * 2, 1, 2), None);
    }

    #[test]
    fn supports_seek_ranges_beyond_32_bit_offsets() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let start = 5 * GIB + 17;
        let end = start + 4095;
        assert_eq!(
            parse_byte_range(Some(&format!("bytes={start}-{end}")), 6 * GIB),
            Ok((start, end, true))
        );

        let local_start = start % CHUNK_SIZE;
        assert_eq!(
            next_chunk_window(start, 4096, 60),
            Some(ChunkWindow {
                index: usize::try_from(start / CHUNK_SIZE).unwrap(),
                local_start,
                local_end: local_start + 4095,
                length: 4096,
            })
        );
    }

    #[test]
    fn prioritizes_start_tail_and_timeline_regions_during_link_warmup() {
        assert!(direct_link_warm_order(0).is_empty());
        assert_eq!(direct_link_warm_order(1), vec![0]);

        let order = direct_link_warm_order(10);
        assert_eq!(&order[..4], &[0, 9, 1, 5]);
        let mut sorted = order;
        sorted.sort_unstable();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn warms_the_chunk_after_a_random_seek() {
        assert_eq!(next_seek_chunk_to_warm(0, 4), Some(1));
        assert_eq!(next_seek_chunk_to_warm(CHUNK_SIZE * 2 + 17, 4), Some(3));
        assert_eq!(next_seek_chunk_to_warm(CHUNK_SIZE * 3, 4), None);
        assert_eq!(next_seek_chunk_to_warm(0, 1), None);
    }

    #[test]
    fn direct_links_refresh_before_they_expire() {
        let now = Instant::now();
        let entry = CachedDirectUrl {
            url: "https://cdn.invalid/file".to_string(),
            refresh_at: now + Duration::from_secs(10),
            expires_at: now + Duration::from_secs(20),
        };
        assert!(entry.is_usable_at(now + Duration::from_secs(15)));
        assert!(entry.needs_refresh_at(now + Duration::from_secs(15)));
        assert!(!entry.is_usable_at(now + Duration::from_secs(20)));
    }

    #[test]
    fn bounds_stream_recovery_retries_and_link_refreshes() {
        let mut transport = StreamRecovery::default();
        assert_eq!(transport.transport_failure(), RecoveryAction::RetrySameUrl);
        assert_eq!(transport.transport_failure(), RecoveryAction::RefreshLink);
        assert_eq!(transport.transport_failure(), RecoveryAction::RetrySameUrl);
        assert_eq!(transport.transport_failure(), RecoveryAction::Fail);

        let mut rejection = StreamRecovery::default();
        assert_eq!(rejection.rejected_link(), RecoveryAction::RefreshLink);
        assert_eq!(rejection.rejected_link(), RecoveryAction::Fail);
    }

    #[test]
    fn rejects_cloud_error_payloads_without_rejecting_generic_media() {
        assert!(upstream_content_type_is_rejected(
            "text/html; charset=utf-8",
            "video/mp4"
        ));
        assert!(upstream_content_type_is_rejected(
            "application/json",
            "video/mp4"
        ));
        assert!(upstream_content_type_is_rejected(
            "application/problem+json",
            "audio/mpeg"
        ));
        assert!(!upstream_content_type_is_rejected(
            "application/octet-stream",
            "video/mp4"
        ));
        assert!(!upstream_content_type_is_rejected(
            "application/json; charset=utf-8",
            "application/json"
        ));
        assert!(!upstream_content_type_is_rejected(
            "text/html; charset=utf-8",
            "text/html; charset=utf-8"
        ));
        assert!(!upstream_content_type_is_rejected("video/mp4", "video/mp4"));
        assert!(!upstream_content_type_is_rejected("", "video/mp4"));
    }

    #[test]
    fn maps_advertised_media_and_document_content_types() {
        assert_eq!(content_type_for_name("movie.mpeg"), "video/mpeg");
        assert_eq!(content_type_for_name("audio.wma"), "audio/x-ms-wma");
        assert_eq!(content_type_for_name("data.json"), "application/json");
        assert_eq!(
            content_type_for_name("page.html"),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type_for_name("unknown.bin"),
            "application/octet-stream"
        );
    }

    #[tokio::test]
    async fn replacement_source_is_isolated_from_stale_background_work() {
        const VFS_ID: u64 = u64::MAX - 10;
        let original_access = Instant::now() - Duration::from_secs(30);
        get_cache().lock().await.insert(
            VFS_ID,
            CachedMedia {
                source_key: "replacement".to_string(),
                chunks_str: "2".to_string(),
                total_size: CHUNK_SIZE * 2,
                urls: vec!["share-a".to_string(), "share-b".to_string()],
                chunk_password: None,
                direct_urls: vec![
                    Some(CachedDirectUrl::new("replacement-url".to_string())),
                    None,
                ],
                last_accessed_at: original_access,
            },
        );

        assert_eq!(cached_direct_url(VFS_ID, "stale", 0).await, None);
        assert!(direct_url_needs_refresh(VFS_ID, "stale", 0).await);
        store_direct_url(VFS_ID, "stale", 0, "stale-url".to_string()).await;
        invalidate_direct_url(VFS_ID, "stale", 0, "stale-url").await;
        touch_cached_media(VFS_ID, "stale").await;

        assert_eq!(
            cached_direct_url(VFS_ID, "replacement", 0).await,
            Some("replacement-url".to_string())
        );
        assert_eq!(
            get_cache().lock().await[&VFS_ID].last_accessed_at,
            original_access
        );
        let stale_lock = direct_resolve_lock(VFS_ID, "stale", 0).await;
        let replacement_lock = direct_resolve_lock(VFS_ID, "replacement", 0).await;
        assert!(!std::sync::Arc::ptr_eq(&stale_lock, &replacement_lock));
        let stale_media_lock = media_resolve_lock(VFS_ID, "stale").await;
        let replacement_media_lock = media_resolve_lock(VFS_ID, "replacement").await;
        assert!(!std::sync::Arc::ptr_eq(
            &stale_media_lock,
            &replacement_media_lock
        ));

        get_cache().lock().await.remove(&VFS_ID);
    }

    #[tokio::test]
    async fn stale_failure_cannot_erase_a_refreshed_direct_link() {
        const VFS_ID: u64 = u64::MAX - 11;
        get_cache().lock().await.insert(
            VFS_ID,
            CachedMedia {
                source_key: "source".to_string(),
                chunks_str: "2".to_string(),
                total_size: CHUNK_SIZE * 2,
                urls: vec!["share-a".to_string(), "share-b".to_string()],
                chunk_password: None,
                direct_urls: vec![
                    Some(CachedDirectUrl::new("refreshed-url".to_string())),
                    None,
                ],
                last_accessed_at: Instant::now(),
            },
        );

        invalidate_direct_url(VFS_ID, "source", 0, "stale-url").await;
        assert_eq!(
            cached_direct_url(VFS_ID, "source", 0).await,
            Some("refreshed-url".to_string())
        );

        invalidate_direct_url(VFS_ID, "source", 0, "refreshed-url").await;
        assert_eq!(cached_direct_url(VFS_ID, "source", 0).await, None);
        get_cache().lock().await.remove(&VFS_ID);
    }

    #[test]
    fn selects_a_physical_private_lan_over_virtual_addresses() {
        let interfaces = vec![
            (
                "Mihomo Tunnel".to_string(),
                "198.18.0.1".parse::<IpAddr>().unwrap(),
            ),
            (
                "VMware Network Adapter".to_string(),
                "192.168.121.1".parse::<IpAddr>().unwrap(),
            ),
            (
                "Wi-Fi".to_string(),
                "192.168.1.27".parse::<IpAddr>().unwrap(),
            ),
            (
                "Loopback".to_string(),
                "127.0.0.1".parse::<IpAddr>().unwrap(),
            ),
        ];

        assert_eq!(
            select_lan_ipv4(interfaces),
            Some(("Wi-Fi".to_string(), Ipv4Addr::new(192, 168, 1, 27)))
        );
    }

    #[test]
    fn does_not_advertise_benchmark_or_carrier_nat_addresses() {
        let interfaces = vec![
            ("Proxy".to_string(), "198.18.0.1".parse::<IpAddr>().unwrap()),
            (
                "Carrier".to_string(),
                "100.64.0.5".parse::<IpAddr>().unwrap(),
            ),
        ];

        assert_eq!(select_lan_ipv4(interfaces), None);
    }
}

fn content_type_for_name(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mpeg" | "mpg" => "video/mpeg",
        "ts" | "m2ts" => "video/mp2t",
        "ogv" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "wma" => "audio/x-ms-wma",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" | "c" | "cpp" | "rs" | "py" | "js" | "md" | "log" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "html" | "htm" => "text/html; charset=utf-8",
        "xls" => "application/vnd.ms-excel",
        "doc" => "application/msword",
        "ppt" => "application/vnd.ms-powerpoint",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
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
    let active_port = *get_server_port().lock().await;
    let config_arc = get_config();
    let mut config = config_arc.lock().await;
    config.port = active_port.unwrap_or(port);
    config.username = username;
    config.password = password;
    Ok(())
}

#[tauri::command]
pub async fn boot_webdav_server(
    port: u16,
    username: String,
    password: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u16, String> {
    let server_port = get_server_port();
    let mut running_port = server_port.lock().await;

    if let Some(active_port) = *running_port {
        let config_arc = get_config();
        let mut config = config_arc.lock().await;
        config.port = active_port;
        config.username = username;
        config.password = password;
        println!("[WEBDAV] Server is already ready on 0.0.0.0:{active_port}");
        return Ok(active_port);
    }

    {
        let config_arc = get_config();
        let mut config = config_arc.lock().await;
        config.port = port;
        config.username = username;
        config.password = password;
    }

    // Bind before reporting success. The previous detached bind could panic after this command
    // returned Ok and permanently leave the process believing that its server was running.
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
        .await
        .map_err(|error| {
            format!("Unable to bind WebDAV/stream server to 0.0.0.0:{port}: {error}")
        })?;
    let bound_port = listener
        .local_addr()
        .map_err(|error| format!("Unable to inspect WebDAV listener: {error}"))?
        .port();
    *running_port = Some(bound_port);
    drop(running_port);

    let app_state = state.inner().clone();
    tokio::spawn(async move {
        run_server(listener, app_state, app).await;
    });

    Ok(bound_port)
}

#[tauri::command]
pub async fn prepare_media_stream(vfs_id: u64, port: u16) -> Result<String, String> {
    let running_port = *get_server_port().lock().await;
    if running_port != Some(port) {
        return Err(format!(
            "Local stream server is not running on the requested port {port}"
        ));
    }

    let uri = format!("http://127.0.0.1:{port}/stream/{vfs_id}");
    println!("[STREAM] Preflighting VFS {vfs_id} through {uri}");
    let response = tokio::time::timeout(
        CLOUD_RESOLVE_TIMEOUT + Duration::from_secs(15),
        get_stream_client()
            .get(&uri)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .timeout(CLOUD_RESOLVE_TIMEOUT)
            .send(),
    )
    .await
    .map_err(|_| format!("Timed out preparing media stream for VFS node {vfs_id}"))?
    .map_err(|error| format!("Unable to reach the local media stream: {error}"))?;

    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(format!(
            "Media stream preparation failed with HTTP {status}: {detail}"
        ));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Unable to read the media preflight byte: {error}"))?;
    if bytes.len() != 1 {
        return Err(format!(
            "Media stream preflight returned {} bytes instead of one",
            bytes.len()
        ));
    }
    println!("[STREAM] VFS {vfs_id} is ready for native playback");
    Ok(uri)
}

#[tauri::command]
pub fn get_local_ip() -> String {
    match select_lan_ipv4(local_ip_address::list_afinet_netifas().unwrap_or_default()) {
        Some((interface, address)) => {
            println!("[WEBDAV] Advertising LAN interface '{interface}' at {address}");
            address.to_string()
        }
        None => {
            eprintln!("[WEBDAV] No phone-reachable private IPv4 interface was found");
            "LAN address unavailable".to_string()
        }
    }
}

fn select_lan_ipv4(interfaces: Vec<(String, IpAddr)>) -> Option<(String, Ipv4Addr)> {
    interfaces
        .into_iter()
        .filter_map(|(name, address)| match address {
            IpAddr::V4(address) => {
                lan_address_score(&name, address).map(|score| (score, name, address))
            }
            IpAddr::V6(_) => None,
        })
        .max_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| right.2.octets().cmp(&left.2.octets()))
        })
        .map(|(_, name, address)| (name, address))
}

fn lan_address_score(interface: &str, address: Ipv4Addr) -> Option<i32> {
    let [a, b, _, _] = address.octets();
    // Only advertise RFC 1918 addresses. In particular, 198.18.0.0/15 is reserved for
    // benchmarking and is commonly used by VPN/proxy virtual adapters; a phone on the physical
    // LAN cannot route to it. Link-local, loopback, multicast, and carrier-NAT addresses are also
    // inappropriate for a WebDAV URL intended for another device.
    let mut score = match (a, b) {
        (192, 168) => 300,
        (10, _) => 240,
        (172, 16..=31) => 220,
        _ => return None,
    };

    let name = interface.to_ascii_lowercase();
    if [
        "wi-fi", "wifi", "wlan", "wireless", "ethernet", "en0", "en1", "eth0", "eth1",
    ]
    .iter()
    .any(|marker| name.contains(marker))
    {
        score += 100;
    }
    if [
        "vpn",
        "tun",
        "tap",
        "virtual",
        "vmware",
        "hyper-v",
        "vethernet",
        "vbox",
        "docker",
        "wsl",
        "zerotier",
        "tailscale",
        "utun",
        "bridge",
    ]
    .iter()
    .any(|marker| name.contains(marker))
    {
        score -= 250;
    }
    Some(score)
}

async fn handle_mcp_http(
    AxumState(_state): AxumState<Arc<AppState>>,
    axum::Extension(app): axum::Extension<tauri::AppHandle>,
    axum::Json(req): axum::Json<crate::mcp::JsonRpcRequest>,
) -> impl IntoResponse {
    let response = crate::mcp::handle_mcp_request(req, app).await;
    (StatusCode::OK, axum::Json(response))
}

fn get_stream_client() -> Client {
    STREAM_CLIENT
        .get_or_init(|| {
            Client::builder()
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                .pool_idle_timeout(Duration::from_secs(90))
                .pool_max_idle_per_host(8)
                .tcp_keepalive(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(12))
                .build()
                .expect("failed to build streaming HTTP client")
        })
        .clone()
}

async fn cached_direct_url(vfs_id: u64, source_key: &str, chunk_index: usize) -> Option<String> {
    let cache = get_cache();
    let lock = cache.lock().await;
    let now = Instant::now();
    lock.get(&vfs_id)
        .filter(|media| media.source_key == source_key)
        .and_then(|media| media.direct_urls.get(chunk_index))
        .and_then(Option::as_ref)
        .filter(|entry| entry.is_usable_at(now))
        .map(|entry| entry.url.clone())
}

async fn cache_contains_source(vfs_id: u64, source_key: &str) -> bool {
    get_cache()
        .lock()
        .await
        .get(&vfs_id)
        .is_some_and(|media| media.source_key == source_key)
}

async fn direct_url_needs_refresh(vfs_id: u64, source_key: &str, chunk_index: usize) -> bool {
    let cache = get_cache();
    let lock = cache.lock().await;
    let now = Instant::now();
    lock.get(&vfs_id)
        .filter(|media| media.source_key == source_key)
        .and_then(|media| media.direct_urls.get(chunk_index))
        .and_then(Option::as_ref)
        .map(|entry| entry.needs_refresh_at(now))
        .unwrap_or(true)
}

async fn store_direct_url(vfs_id: u64, source_key: &str, chunk_index: usize, url: String) {
    let cache = get_cache();
    let mut lock = cache.lock().await;
    if let Some(media) = lock
        .get_mut(&vfs_id)
        .filter(|media| media.source_key == source_key)
    {
        if chunk_index < media.direct_urls.len() {
            media.direct_urls[chunk_index] = Some(CachedDirectUrl::new(url));
        }
    }
}

async fn invalidate_direct_url(
    vfs_id: u64,
    source_key: &str,
    chunk_index: usize,
    failed_url: &str,
) {
    let cache = get_cache();
    let mut lock = cache.lock().await;
    if let Some(media) = lock
        .get_mut(&vfs_id)
        .filter(|media| media.source_key == source_key)
    {
        if let Some(entry) = media.direct_urls.get_mut(chunk_index) {
            if entry
                .as_ref()
                .is_some_and(|cached| cached.url == failed_url)
            {
                *entry = None;
            }
        }
    }
}

async fn resolve_cached_chunk_link(
    vfs_id: u64,
    source_key: &str,
    chunk_index: usize,
    downloader: &crate::lanzou_down::LanzouDownloader,
    share_url: &str,
    password: Option<&str>,
) -> Result<String, String> {
    if let Some(url) = cached_direct_url(vfs_id, source_key, chunk_index).await {
        return Ok(url);
    }
    let resolve_lock = direct_resolve_lock(vfs_id, source_key, chunk_index).await;
    let _resolve_guard = resolve_lock.lock().await;
    if let Some(url) = cached_direct_url(vfs_id, source_key, chunk_index).await {
        return Ok(url);
    }
    println!(
        "[STREAM] VFS {vfs_id}: resolving direct URL for chunk {}",
        chunk_index + 1
    );
    let url = resolve_direct_link_bounded(downloader, share_url, password).await?;
    store_direct_url(vfs_id, source_key, chunk_index, url.clone()).await;
    Ok(url)
}

async fn refresh_cached_chunk_link(
    vfs_id: u64,
    source_key: &str,
    chunk_index: usize,
    downloader: &crate::lanzou_down::LanzouDownloader,
    share_url: &str,
    password: Option<&str>,
) -> Result<String, String> {
    let resolve_lock = direct_resolve_lock(vfs_id, source_key, chunk_index).await;
    let _resolve_guard = resolve_lock.lock().await;
    if !cache_contains_source(vfs_id, source_key).await {
        return Err(MEDIA_SOURCE_REPLACED.to_string());
    }
    if !direct_url_needs_refresh(vfs_id, source_key, chunk_index).await {
        return cached_direct_url(vfs_id, source_key, chunk_index)
            .await
            .ok_or_else(|| "Direct-link cache changed during refresh".to_string());
    }
    let url = resolve_direct_link_bounded(downloader, share_url, password).await?;
    store_direct_url(vfs_id, source_key, chunk_index, url.clone()).await;
    Ok(url)
}

fn warm_seek_neighbor_link(
    vfs_id: u64,
    source_key: String,
    downloader: crate::lanzou_down::LanzouDownloader,
    share_url: String,
    password: Option<String>,
    chunk_index: usize,
) {
    tokio::spawn(async move {
        let result = refresh_cached_chunk_link(
            vfs_id,
            &source_key,
            chunk_index,
            &downloader,
            &share_url,
            password.as_deref(),
        )
        .await;
        if let Err(error) = result {
            if error != MEDIA_SOURCE_REPLACED {
                eprintln!(
                    "[STREAM] Seek-neighbor direct-link warmup failed for VFS {vfs_id} chunk {}: {error}",
                    chunk_index + 1
                );
            }
        }
    });
}

async fn warm_direct_links(
    vfs_id: u64,
    source_key: String,
    downloader: crate::lanzou_down::LanzouDownloader,
    share_urls: Vec<String>,
    password: Option<String>,
) {
    if !cache_contains_source(vfs_id, &source_key).await {
        return;
    }
    let warming = WARMING_DIRECT_LINKS
        .get_or_init(|| Arc::new(Mutex::new(HashSet::new())))
        .clone();
    let semaphore = DIRECT_LINK_WARM_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(DIRECT_LINK_WARM_CONCURRENCY)))
        .clone();

    for chunk_index in direct_link_warm_order(share_urls.len()) {
        let share_url = share_urls[chunk_index].clone();
        if !direct_url_needs_refresh(vfs_id, &source_key, chunk_index).await {
            continue;
        }
        {
            let mut active = warming.lock().await;
            if !active.insert((vfs_id, source_key.clone(), chunk_index)) {
                continue;
            }
        }

        let warming = warming.clone();
        let semaphore = semaphore.clone();
        let downloader = downloader.clone();
        let password = password.clone();
        let task_source_key = source_key.clone();
        tokio::spawn(async move {
            let result = match semaphore.acquire_owned().await {
                Ok(_permit) => {
                    if !cache_contains_source(vfs_id, &task_source_key).await {
                        Err(MEDIA_SOURCE_REPLACED.to_string())
                    } else {
                        refresh_cached_chunk_link(
                            vfs_id,
                            &task_source_key,
                            chunk_index,
                            &downloader,
                            &share_url,
                            password.as_deref(),
                        )
                        .await
                    }
                }
                Err(_) => Err("Direct-link warmup semaphore closed".to_string()),
            };
            if let Err(error) = result {
                if error != MEDIA_SOURCE_REPLACED {
                    eprintln!(
                        "[STREAM] Background direct-link refresh failed for VFS {vfs_id} chunk {}: {error}",
                        chunk_index + 1
                    );
                }
            }
            warming
                .lock()
                .await
                .remove(&(vfs_id, task_source_key, chunk_index));
        });
    }
}

async fn touch_cached_media(vfs_id: u64, source_key: &str) {
    let cache = get_cache();
    let mut cache = cache.lock().await;
    if let Some(media) = cache
        .get_mut(&vfs_id)
        .filter(|media| media.source_key == source_key)
    {
        media.last_accessed_at = Instant::now();
    }
}

async fn ensure_direct_link_refresh_supervisor(
    vfs_id: u64,
    source_key: String,
    downloader: crate::lanzou_down::LanzouDownloader,
    share_urls: Vec<String>,
    password: Option<String>,
) {
    let supervisors = ACTIVE_REFRESH_SUPERVISORS
        .get_or_init(|| Arc::new(Mutex::new(HashSet::new())))
        .clone();
    {
        let mut active = supervisors.lock().await;
        if !active.insert((vfs_id, source_key.clone())) {
            return;
        }
    }

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(DIRECT_LINK_REFRESH_POLL).await;
            let should_continue = {
                let cache = get_cache();
                let cache = cache.lock().await;
                cache
                    .get(&vfs_id)
                    .filter(|media| media.source_key == source_key)
                    .map(|media| media.last_accessed_at.elapsed() < MEDIA_ACTIVE_WINDOW)
                    .unwrap_or(false)
            };
            if !should_continue {
                break;
            }
            warm_direct_links(
                vfs_id,
                source_key.clone(),
                downloader.clone(),
                share_urls.clone(),
                password.clone(),
            )
            .await;
        }
        supervisors.lock().await.remove(&(vfs_id, source_key));
    });
}
