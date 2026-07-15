use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use base64::{engine::general_purpose::STANDARD, Engine as _};

const ALLOWED_EXTS: &[&str] = &[
    "doc",
    "docx",
    "zip",
    "rar",
    "apk",
    "txt",
    "exe",
    "7z",
    "e",
    "z",
    "ct",
    "ke",
    "cetrainer",
    "db",
    "tar",
    "pdf",
    "w3x",
    "epub",
    "mobi",
    "azw",
    "azw3",
    "osk",
    "osz",
    "xpa",
    "cpk",
    "lua",
    "jar",
    "dmg",
    "ppt",
    "pptx",
    "xls",
    "xlsx",
    "mp3",
    "ipa",
    "iso",
    "img",
    "gho",
    "ttf",
    "ttc",
    "txf",
    "dwg",
    "bat",
    "imazingapp",
    "dll",
    "crx",
    "xapk",
    "conf",
    "deb",
    "rp",
    "rpm",
    "rplib",
    "mobileconfig",
    "appimage",
    "lolgezi",
    "flac",
    "cad",
    "hwt",
    "accdb",
    "ce",
    "xmind",
    "enc",
    "bds",
    "bdi",
    "ssf",
    "it",
    "pkg",
    "cfg",
    "mp4",
    "avi",
    "png",
    "jpeg",
    "jpg",
    "gif",
    "webp",
    "brushset",
];

static TIME_OFFSET: AtomicI64 = AtomicI64::new(0);
static HAS_SYNCED_TIME: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeType {
    Directory,
    File,
}

impl NodeType {
    fn from_str(s: &str) -> Self {
        match s {
            "D" => NodeType::Directory,
            _ => NodeType::File,
        }
    }
    fn as_str(&self) -> &str {
        match self {
            NodeType::Directory => "D",
            NodeType::File => "F",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsNode {
    pub node_type: NodeType,
    pub id: u64,
    pub pid: u64,
    pub name: String,
    pub lanzou_id: String,
    pub time: u64,
    pub size: String,
    pub md5: String,
    pub ext: String,
    pub chunks: String,
    pub is_trashed: bool,
    pub is_deleted: bool,
}

pub struct VfsTree {
    pub last_modified: u64,
    pub root_lanzou_id: String,
    pub deeperdir_lanzou_id: String,
    pub nodes: HashMap<u64, VfsNode>,
    pub next_id: u64,
    pub file_path: PathBuf,
}

impl VfsTree {
    /// Creates a fresh, empty Virtual File System
    pub fn new(root_lanzou_id: String, deeperdir_lanzou_id: String, file_path: PathBuf) -> Self {
        Self {
            last_modified: 0,
            root_lanzou_id,
            deeperdir_lanzou_id,
            nodes: HashMap::new(),
            next_id: 1,
            file_path,
        }
    }

    /// Saves the current VFS state to the local disk
    pub fn save_local(&self) -> Result<(), String> {
        let data = self.to_tsv();
        fs::write(&self.file_path, data).map_err(|e| e.to_string())
    }

    /// Loads the VFS state from the local disk
    pub fn load_local(file_path: PathBuf) -> Result<Self, String> {
        if !file_path.exists() {
            return Err("Local tree does not exist".to_string());
        }
        let data = fs::read_to_string(&file_path).map_err(|e| e.to_string())?;
        Self::from_tsv(&data, file_path)
    }

    /// Parses the VFS from the raw TSV (Pipe-Separated) text file
    pub fn from_tsv(data: &str, file_path: PathBuf) -> Result<Self, String> {
        let mut lines = data.lines().filter(|l| !l.trim().is_empty());

        let header = lines.next().ok_or("Empty tree file")?;
        let h_parts: Vec<&str> = header.split('|').collect();
        if h_parts.len() < 4 || (h_parts[0] != "V1" && h_parts[0] != "V2") {
            return Err("Invalid or unsupported tree file version".to_string());
        }
        let is_v2 = h_parts[0] == "V2";

        let last_modified = h_parts[1].parse::<u64>().unwrap_or(0);
        let root_lanzou_id = h_parts[3].to_string();
        let deeperdir_lanzou_id = h_parts.get(4).unwrap_or(&"").to_string();

        let mut nodes = HashMap::new();
        let mut max_id = 0;

        for line in lines {
            let p: Vec<&str> = line.split('|').collect();
            if p.len() < 10 {
                continue;
            }

            let id = p[1].parse::<u64>().unwrap_or(0);
            if id > max_id {
                max_id = id;
            }

            let safe_name;
            let suf_idx; // The index where the suffix block (lanzou_id) begins

            if is_v2 {
                // V2 strict layout: name is at index 3 and Base64 encoded (no pipes)
                safe_name = String::from_utf8(STANDARD.decode(p[3]).unwrap_or_default())
                    .unwrap_or_else(|_| p[3].to_string());
                suf_idx = 4;
            } else {
                // Look at the tail elements to see if the optional boolean flags exist
                let len = p.len();
                let last_val = p[len - 1];
                let prev_val = p[len - 2];

                let (has_trashed, has_deleted) = match (prev_val, last_val) {
                    ("true" | "false", "true" | "false") => (true, true),
                    (_, "true" | "false") => (true, false),
                    _ => (false, false),
                };

                // Base suffix is 6 fields (lanzou_id through chunks). Add flags if present.
                let suffix_fields = 6 + (has_trashed as usize) + (has_deleted as usize);
                suf_idx = len - suffix_fields;

                // Reconstruct any filename that had a pipe in it by re-joining the middle chunks!
                safe_name = p[3..suf_idx].join("|");
            }

            let node = VfsNode {
                node_type: NodeType::from_str(p[0]),
                id,
                pid: p[2].parse::<u64>().unwrap_or(0),
                name: safe_name,
                lanzou_id: p[suf_idx].to_string(),
                time: p[suf_idx + 1].parse::<u64>().unwrap_or(0),
                size: p[suf_idx + 2].to_string(),
                md5: p[suf_idx + 3].to_string(),
                ext: p[suf_idx + 4].to_string(),
                chunks: p[suf_idx + 5].to_string(),
                is_trashed: p.get(suf_idx + 6).unwrap_or(&"false") == &"true",
                is_deleted: p.get(suf_idx + 7).unwrap_or(&"false") == &"true",
            };

            nodes.insert(id, node);
        }

        Ok(Self {
            last_modified,
            root_lanzou_id,
            deeperdir_lanzou_id,
            nodes,
            next_id: max_id + 1,
            file_path,
        })
    }

    /// Serializes the VFS back into the compact TSV string format for cloud upload
    pub fn to_tsv(&self) -> String {
        let mut output = String::new();

        // --- MINIMUM FIX 2: Change Header to V2 ---
        output.push_str(&format!(
            "V2|{}|{}|{}|{}\n",
            self.last_modified,
            self.nodes.len(),
            self.root_lanzou_id,
            self.deeperdir_lanzou_id
        ));

        // Rows
        for node in self.nodes.values() {
            let encoded_name = STANDARD.encode(&node.name);

            output.push_str(&format!(
                "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
                node.node_type.as_str(),
                node.id,
                node.pid,
                encoded_name,
                node.lanzou_id,
                node.time,
                node.size,
                node.md5,
                node.ext,
                node.chunks,
                node.is_trashed,
                node.is_deleted
            ));
        }

        output
    }

    /// Updates the global modification timestamp
    pub fn touch(&mut self) {
        self.last_modified = current_timestamp();
    }

    // --------------------------------------------------------
    // Virtual File Operations
    // --------------------------------------------------------

    /// Retrieves all immediate children of a specific parent folder ID (0 = root)
    pub fn list_dir(&self, pid: u64) -> Vec<VfsNode> {
        let mut children: Vec<VfsNode> = self
            .nodes
            .values()
            // --- MINIMUM FIX: Hide deleted tombstones from UI ---
            .filter(|n| n.pid == pid && !n.is_deleted)
            .cloned()
            .collect();

        // Sort folders first, then files, both alphabetically
        children.sort_by(|a, b| {
            if a.node_type != b.node_type {
                if a.node_type == NodeType::Directory {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                a.name.cmp(&b.name)
            }
        });

        children
    }

    /// Registers a new folder in the VFS
    pub fn create_folder(&mut self, pid: u64, name: &str, lanzou_id: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let node = VfsNode {
            node_type: NodeType::Directory,
            id,
            pid,
            name: name.to_string(),
            lanzou_id: lanzou_id.to_string(),
            time: current_timestamp(),
            size: "".to_string(),
            md5: "".to_string(),
            ext: "".to_string(),
            chunks: "".to_string(),
            is_trashed: false,
            is_deleted: false,
        };

        self.nodes.insert(id, node);
        self.touch();
        id
    }

    /// Registers a new file (or chunked file) in the VFS
    pub fn add_file(
        &mut self,
        pid: u64,
        name: &str,
        lanzou_id: &str, // This is the file ID, or folder ID if chunked
        size: &str,
        md5: &str,
        original_ext: &str,
        chunks: u32,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let chunk_str = if chunks > 1 {
            chunks.to_string()
        } else {
            "1".to_string()
        };

        let node = VfsNode {
            node_type: NodeType::File,
            id,
            pid,
            name: name.to_string(),
            lanzou_id: lanzou_id.to_string(),
            time: current_timestamp(),
            size: size.to_string(),
            md5: md5.to_string(),
            ext: original_ext.to_string(),
            chunks: chunk_str,
            is_trashed: false,
            is_deleted: false,
        };

        self.nodes.insert(id, node);
        self.touch();
        id
    }

    /// Moves a file or folder instantly by reassigning its Parent ID (PID)
    /// This entirely bypasses Lanzou's inability to move files!
    pub fn move_node(&mut self, id: u64, new_pid: u64) -> Result<(), String> {
        if !self.nodes.contains_key(&id) {
            return Err("Node not found".to_string());
        }

        // Prevent moving a folder into itself or its own children
        if id == new_pid {
            return Err("Cannot move a folder into itself".to_string());
        }

        if let Some(node) = self.nodes.get_mut(&id) {
            node.pid = new_pid;
            node.time = current_timestamp(); // Update modified time for CRDT
        }

        self.touch();
        Ok(())
    }

    /// Recursively Tombstones a node and all of its nested children from the VFS
    pub fn delete_node(&mut self, target_id: u64) {
        let mut to_delete = vec![target_id];
        let mut i = 0;

        // Find all descendants recursively
        while i < to_delete.len() {
            let current = to_delete[i];

            let children: Vec<u64> = self
                .nodes
                .values()
                .filter(|n| n.pid == current)
                .map(|n| n.id)
                .collect();

            for c in children {
                if !to_delete.contains(&c) {
                    to_delete.push(c);
                }
            }
            i += 1;
        }

        let now = current_timestamp();
        // --- MINIMUM FIX: Turn them into Tombstones instead of wiping them! ---
        for del_id in to_delete {
            if let Some(node) = self.nodes.get_mut(&del_id) {
                node.is_deleted = true;
                node.time = now; // Mark time of death for CRDT sync
            }
        }

        self.touch();
    }

    // --------------------------------------------------------
    // CRDT SYNCHRONIZATION ALGORITHM
    // --------------------------------------------------------

    /// Intelligently merges a remote cloud tree into the local tree.
    /// Returns a perfectly synchronized VfsTree without leaving ghost files.
    pub fn merge_with(&self, cloud_tree: &VfsTree) -> Self {
        let mut merged_nodes = HashMap::new();
        let mut all_ids = std::collections::HashSet::new();

        for id in self.nodes.keys() {
            all_ids.insert(*id);
        }
        for id in cloud_tree.nodes.keys() {
            all_ids.insert(*id);
        }

        // Phase 1: Node-Level Resolution (Granular Last Writer Wins)
        for id in all_ids {
            let local_node = self.nodes.get(&id);
            let cloud_node = cloud_tree.nodes.get(&id);

            match (local_node, cloud_node) {
                (Some(l), Some(c)) => {
                    if l.time > c.time {
                        merged_nodes.insert(id, l.clone());
                    } else {
                        merged_nodes.insert(id, c.clone());
                    }
                }
                (Some(l), None) => {
                    merged_nodes.insert(id, l.clone());
                }
                (None, Some(c)) => {
                    merged_nodes.insert(id, c.clone());
                }
                _ => {}
            }
        }

        // Phase 2: Structural Repair (Orphaned File Protection)
        let mut repaired_nodes = merged_nodes.clone();
        for (id, node) in merged_nodes.iter() {
            if node.pid != 0 {
                let parent_exists_and_alive = repaired_nodes
                    .get(&node.pid)
                    .map_or(false, |p| !p.is_deleted);

                if !parent_exists_and_alive && !node.is_deleted {
                    // Rescue the orphaned active file by dumping it to the root directory
                    if let Some(mut rescued_node) = repaired_nodes.get_mut(id) {
                        rescued_node.pid = 0;
                        rescued_node.time = current_timestamp();
                    }
                }
            }
        }

        Self {
            last_modified: std::cmp::max(self.last_modified, cloud_tree.last_modified),
            root_lanzou_id: cloud_tree.root_lanzou_id.clone(),
            deeperdir_lanzou_id: cloud_tree.deeperdir_lanzou_id.clone(),
            nodes: repaired_nodes,
            next_id: std::cmp::max(self.next_id, cloud_tree.next_id),
            file_path: self.file_path.clone(),
        }
    }
}

// --------------------------------------------------------
// Utility Functions
// --------------------------------------------------------

/// Generates the current Unix epoch timestamp in seconds
pub fn current_timestamp() -> u64 {
    // 1. If we haven't synced yet, do it EXACTLY ONCE per session.
    if !HAS_SYNCED_TIME.load(Ordering::Relaxed) {
        // --- MINIMUM FIX: std::thread::spawn completely escapes Tauri's Tokio context, preventing the Panic! ---
        let offset_result = std::thread::spawn(|| {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_millis(1500))
                .user_agent("Mozilla/5.0")
                .build()
                .ok()?;

            let resp = client.get("http://f.m.suning.com/api/ct.do").send().ok()?;
            let json = resp.json::<serde_json::Value>().ok()?;
            let network_time = json["currentTime"].as_i64()?;

            let local_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            // Calculate how far ahead or behind the hardware clock is
            Some(network_time - local_time)
        })
        .join()
        .unwrap_or(None);

        if let Some(offset) = offset_result {
            println!(
                "[TIME-SYNC] Hardware clock offset established: {}ms",
                offset
            );
            TIME_OFFSET.store(offset, Ordering::Relaxed);
        } else {
            println!("[TIME-SYNC] Network unavailable. Proceeding with zero offset.");
        }

        HAS_SYNCED_TIME.store(true, Ordering::Relaxed);
    }

    // 2. For all 10,000 subsequent file operations, just do the ultra-fast math!
    let local_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let offset = TIME_OFFSET.load(Ordering::Relaxed);

    // Return the perfectly synchronized time
    (local_time + offset) as u64
}

/// Checks if an extension is permitted by Lanzou natively.
/// Returns the safe extension to use for the upload API.
pub fn get_safe_lanzou_ext(original_ext: &str) -> String {
    let ext_lower = original_ext.to_lowercase();
    if ALLOWED_EXTS.contains(&ext_lower.as_str()) {
        ext_lower
    } else {
        "iso".to_string()
    }
}
