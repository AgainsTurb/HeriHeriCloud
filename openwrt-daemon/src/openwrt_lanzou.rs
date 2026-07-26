use crate::openwrt_heriheri::{get_safe_lanzou_ext, NodeType, VfsNode, VfsTree};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};
use regex::Regex;
use reqwest::{header, multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

const BASE_URL: &str = "https://up.woozooo.com";

#[derive(Clone)]
pub struct LanzouCloud {
    pub client: Client,
    pub ylogin: Option<String>,
    pub folder_stack: Vec<String>,
}

/// Pure Rust implementation of the Ali WAF acw_sc__v2 decryption algorithm.
/// Bypasses the need to execute the obfuscated JavaScript.
fn solve_ali_waf(arg1: &str) -> String {
    // The fixed permutation array extracted from the JS
    let m = [
        15, 35, 29, 24, 33, 16, 1, 38, 10, 9, 19, 31, 40, 27, 22, 23, 25, 13, 6, 11, 39, 18, 20, 8,
        14, 21, 32, 26, 2, 30, 7, 4, 17, 5, 3, 28, 34, 37, 12, 36,
    ];
    // The static XOR key used by Ali WAF
    let p = "3000176000856006061501533003690027800375";

    let chars: Vec<char> = arg1.chars().collect();
    let mut q = vec![' '; 40];

    // Step 1: Unshuffle the arg1 string
    for i in 0..chars.len() {
        for j in 0..m.len() {
            if m[j] == i + 1 {
                if i < chars.len() {
                    q[j] = chars[i];
                }
            }
        }
    }
    let u: String = q.into_iter().collect();

    // Step 2: XOR pairs of hex characters
    let mut v = String::new();
    for x in (0..u.len().min(p.len())).step_by(2) {
        let u_hex = &u[x..x + 2];
        let p_hex = &p[x..x + 2];

        let u_val = u8::from_str_radix(u_hex, 16).unwrap_or(0);
        let p_val = u8::from_str_radix(p_hex, 16).unwrap_or(0);

        let xor_val = u_val ^ p_val;
        v.push_str(&format!("{:02x}", xor_val));
    }

    v
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
pub fn encrypt_chunk_filename(_md5_str: &str, chunk_index: u32) -> String {
    let cipher = get_filename_cipher();
    
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    let plaintext = format!("{:04x}", chunk_index);
    let ciphertext = cipher.encrypt(&nonce, plaintext.as_bytes()).expect("Crypto Fail");

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

// Helper function to get the cipher
fn get_cipher() -> ChaCha20Poly1305 {
    let secret = env!("HERIHERI_SECRET_KEY");

    // Ensure the key is exactly 32 bytes
    let mut key_bytes = [0u8; 32];
    let bytes = secret.as_bytes();
    let len = std::cmp::min(bytes.len(), 32);
    key_bytes[..len].copy_from_slice(&bytes[..len]);

    let key = Key::from_slice(&key_bytes);
    ChaCha20Poly1305::new(key)
}

fn encrypt_payload(json_str: &str) -> String {
    let cipher = get_cipher();

    // For share links, we can generate a random 12-byte nonce
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    // Encrypt the payload
    let ciphertext = cipher
        .encrypt(&nonce, json_str.as_bytes())
        .expect("Encryption failure!");

    // Prepend the nonce to the ciphertext so we can decrypt it later
    let mut payload = nonce.to_vec();
    payload.extend_from_slice(&ciphertext);

    // Base64 encode the final binary blob
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    URL_SAFE_NO_PAD.encode(payload)
}

pub fn decrypt_payload(encoded: &str) -> Result<String, String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "Invalid Base64")?;
    if decoded.len() < 12 {
        return Err("Payload too short".into());
    }

    // Split the nonce from the actual ciphertext
    let (nonce_bytes, ciphertext) = decoded.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = get_cipher();

    // This will FAIL if the string was tampered with (Auth Tag verification)
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption or Authentication failed!")?;

    String::from_utf8(plaintext).map_err(|_| "Invalid UTF-8".into())
}

impl LanzouCloud {
    pub fn new() -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            ),
        );
        headers.insert(
            header::REFERER,
            header::HeaderValue::from_static("https://up.woozooo.com/mydisk.php"),
        );

        let client = Client::builder()
            .default_headers(headers)
            .cookie_store(true)
            .build()
            .unwrap();

        Self {
            client,
            ylogin: None,
            folder_stack: vec!["-1".to_string()],
        }
    }

    pub fn set_cookies(&mut self, ylogin: String, phpdisk_info: String) {
        self.ylogin = Some(ylogin.clone());
        let cookie_str = format!("ylogin={}; phpdisk_info={}", ylogin, phpdisk_info);

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            header::HeaderValue::from_str(&cookie_str).unwrap(),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("Mozilla/5.0"),
        );

        self.client = Client::builder()
            .default_headers(headers)
            .cookie_store(true)
            .build()
            .unwrap();
    }

    pub async fn is_logged_in(&self) -> Result<bool, String> {
        let url = format!("{}/mydisk.php", BASE_URL);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let text = resp.text().await.map_err(|e| e.to_string())?;
        Ok(text.contains("退出"))
    }

    pub async fn get_vei(&self) -> Result<String, String> {
        let mut url = format!("{}/mydisk.php?item=files&action=index", BASE_URL);
        if let Some(uid) = &self.ylogin {
            url.push_str(&format!("&u={}", uid));
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let html = resp.text().await.map_err(|e| e.to_string())?;

        let re_direct = Regex::new(r#"['"]vei['"]\s*:\s*['"]([^'"]+)['"]"#).unwrap();
        if let Some(caps) = re_direct.captures(&html) {
            return Ok(caps[1].to_string());
        }

        let re_var = Regex::new(r#"['"]vei['"]\s*:\s*([a-zA-Z0-9_]+)"#).unwrap();
        if let Some(caps) = re_var.captures(&html) {
            let var_name = &caps[1];
            let re_val = Regex::new(&format!(r#"{}\s*=\s*['"]([^'"]+)['"]"#, var_name)).unwrap();
            if let Some(val_caps) = re_val.captures(&html) {
                return Ok(val_caps[1].to_string());
            }
        }

        Err("Could not extract vei token".to_string())
    }

    pub async fn get_formhash(&self) -> Result<String, String> {
        let url = format!("{}/mydisk.php?item=recycle&action=files", BASE_URL);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let html = resp.text().await.map_err(|e| e.to_string())?;

        let re = Regex::new(r#"name="formhash"\s+value="([a-fA-F0-9]+)""#).unwrap();
        if let Some(caps) = re.captures(&html) {
            Ok(caps[1].to_string())
        } else {
            Err("Could not find formhash in recycle bin HTML".to_string())
        }
    }

    async fn post_with_waf(&self, form: &[(&str, &str)]) -> Result<Value, String> {
        let base_url = "https://accounts.woozooo.com/accounts.php";
        let referer_url = "https://accounts.woozooo.com/accounts.php?action=register";

        let get_resp = self
            .client
            .get(referer_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let body_text = get_resp.text().await.unwrap_or_default();
        let mut waf_cookie = String::new();

        // Extract and solve the challenge natively
        if body_text.contains("arg1=") {
            let re = Regex::new(r"arg1='([A-F0-9]+)'").unwrap();
            if let Some(caps) = re.captures(&body_text) {
                waf_cookie = format!("acw_sc__v2={}", solve_ali_waf(&caps[1]));
            }
        }

        let mut post_req = self
            .client
            .post(base_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .header("Accept", "application/json, text/javascript, */*")
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", "https://accounts.woozooo.com")
            .header("Host", "accounts.woozooo.com")
            .header("Referer", referer_url);

        if !waf_cookie.is_empty() {
            post_req = post_req.header("Cookie", waf_cookie);
        }

        let resp = post_req
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;

        Ok(json)
    }

    pub async fn init_vfs_root(&mut self) -> Result<(String, String), String> {
        // Temporarily set stack to root (-1) to search
        let old_stack = self.folder_stack.clone();
        self.folder_stack = vec!["-1".to_string()];

        let folders = self.list_folders().await?;
        let mut root_id = String::new();
        let mut deeper_id = String::new();

        for f in folders {
            let name = f["name"].as_str().unwrap_or("");
            let fid = f["fol_id"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| f["fol_id"].as_u64().map(|n| n.to_string()))
                .unwrap_or_default();

            if name == ".heriheri" {
                root_id = fid.clone();
            } else if name == ".deeperdir" {
                deeper_id = fid.clone();
            }
            if !root_id.is_empty() && !deeper_id.is_empty() {
                break;
            }
        }

        // Create .heriheri if not found
        if root_id.is_empty() {
            println!("[INFO] .heriheri root not found. Creating it...");
            let res = self
                .create_folder(".heriheri".to_string(), "HeriHeri VFS Root".to_string())
                .await?;
            root_id = res["text"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| res["text"].as_u64().map(|n| n.to_string()))
                .unwrap_or_default();
        }

        // Create .deeperdir if not found
        if deeper_id.is_empty() {
            println!("[INFO] .deeperdir overflow not found. Creating it...");
            let res = self
                .create_folder(".deeperdir".to_string(), "HeriHeri Overflow".to_string())
                .await?;
            deeper_id = res["text"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| res["text"].as_u64().map(|n| n.to_string()))
                .unwrap_or_default();
        }

        self.folder_stack = old_stack; // Restore user's location
        Ok((root_id, deeper_id))
    }

    pub async fn list_folders(&self) -> Result<Vec<Value>, String> {
        let folder_id = self
            .folder_stack
            .last()
            .unwrap_or(&"-1".to_string())
            .clone();
        let vei = self.get_vei().await?;

        let mut url = format!("{}/doupload.php", BASE_URL);
        if let Some(uid) = &self.ylogin {
            url.push_str(&format!("?uid={}", uid));
        }

        let form = [("task", "47"), ("folder_id", &folder_id), ("vei", &vei)];
        let resp = self
            .client
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;

        if json["zt"] == 1 {
            Ok(json["text"].as_array().unwrap_or(&vec![]).clone())
        } else {
            Ok(vec![])
        }
    }

    pub async fn list_files(&self) -> Result<Vec<Value>, String> {
        let folder_id = self
            .folder_stack
            .last()
            .unwrap_or(&"-1".to_string())
            .clone();
        let vei = self.get_vei().await?;

        let mut url = format!("{}/doupload.php", BASE_URL);
        if let Some(uid) = &self.ylogin {
            url.push_str(&format!("?uid={}", uid));
        }

        let mut all_files = Vec::new();
        let mut pg = 1;

        loop {
            let pg_str = pg.to_string();
            let form = [
                ("task", "5"),
                ("folder_id", &folder_id),
                ("pg", &pg_str),
                ("vei", &vei),
            ];

            let resp = self
                .client
                .post(&url)
                .form(&form)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let json: Value = resp.json().await.map_err(|e| e.to_string())?;

            if json["zt"] != 1 || json["info"] == 0 {
                break;
            }

            if let Some(text_arr) = json["text"].as_array() {
                if text_arr.is_empty() {
                    break;
                }
                all_files.extend(text_arr.clone());
            } else {
                break;
            }
            pg += 1;
        }
        Ok(all_files)
    }

    pub async fn list_folders_by_id(&self, folder_id: &str) -> Result<Vec<Value>, String> {
        let vei = self.get_vei().await?;
        let mut url = format!("{}/doupload.php", BASE_URL);
        if let Some(uid) = &self.ylogin {
            url.push_str(&format!("?uid={}", uid));
        }

        let form = [("task", "47"), ("folder_id", folder_id), ("vei", &vei)];
        let resp = self
            .client
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;

        if json["zt"] == 1 {
            Ok(json["text"].as_array().unwrap_or(&vec![]).clone())
        } else {
            Ok(vec![])
        }
    }

    pub async fn list_files_by_id(&self, folder_id: &str) -> Result<Vec<Value>, String> {
        let vei = self.get_vei().await?;
        let mut url = format!("{}/doupload.php", BASE_URL);
        if let Some(uid) = &self.ylogin {
            url.push_str(&format!("?uid={}", uid));
        }

        let mut all_files = Vec::new();
        let mut pg = 1;

        loop {
            let pg_str = pg.to_string();
            let form = [
                ("task", "5"),
                ("folder_id", folder_id),
                ("pg", &pg_str),
                ("vei", &vei),
            ];

            let resp = self
                .client
                .post(&url)
                .form(&form)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let json: Value = resp.json().await.map_err(|e| e.to_string())?;

            if json["zt"] != 1 || json["info"] == 0 {
                break;
            }
            if let Some(text_arr) = json["text"].as_array() {
                if text_arr.is_empty() {
                    break;
                }
                all_files.extend(text_arr.clone());
            } else {
                break;
            }
            pg += 1;
        }
        Ok(all_files)
    }

    pub fn enter_folder_by_id(&mut self, folder_id: String) {
        let clean_id = if folder_id.starts_with("fol") {
            folder_id.replace("fol", "")
        } else {
            folder_id
        };
        self.folder_stack.push(clean_id);
    }

    pub fn go_back(&mut self) {
        if self.folder_stack.len() > 1 {
            self.folder_stack.pop();
        }
    }

    pub async fn create_folder(
        &self,
        folder_name: String,
        folder_description: String,
    ) -> Result<Value, String> {
        let url = format!("{}/doupload.php", BASE_URL);
        let current_id = self
            .folder_stack
            .last()
            .unwrap_or(&"-1".to_string())
            .clone();
        let parent_id = if current_id == "-1" {
            "0".to_string()
        } else {
            current_id
        };

        let form = [
            ("task", "2"),
            ("parent_id", &parent_id),
            ("folder_name", &folder_name),
            ("folder_description", &folder_description),
        ];

        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        if json["zt"] == 1 {
            Ok(json)
        } else {
            Err("Failed to create folder".to_string())
        }
    }

    pub async fn delete_file(&self, file_id: String) -> Result<bool, String> {
        let url = format!("{}/doupload.php", BASE_URL);
        let form = [("task", "6"), ("file_id", &file_id)];

        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(json["zt"] == 1)
    }

    pub async fn delete_folder(&self, folder_id: String) -> Result<bool, String> {
        let clean_id = if folder_id.starts_with("fol") {
            folder_id.replace("fol", "")
        } else {
            folder_id
        };
        let url = format!("{}/doupload.php", BASE_URL);
        let form = [("task", "3"), ("folder_id", &clean_id)];

        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(json["zt"] == 1)
    }

    pub async fn get_share_info(&self, id: String, is_folder: bool) -> Result<Value, String> {
        let url = format!("{}/doupload.php", BASE_URL);

        // Use task 18 for folders, 22 for files
        let task = if is_folder { "18" } else { "22" };
        let id_key = if is_folder { "folder_id" } else { "file_id" };

        let form = [("task", task), (id_key, &id)];

        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        if json["zt"] == 1 {
            Ok(json["info"].clone())
        } else {
            Err("Failed to get share info".to_string())
        }
    }

    pub async fn move_item(
        &self,
        item_id: String,
        target_folder_id: String,
    ) -> Result<bool, String> {
        let url = format!("{}/doupload.php", BASE_URL);

        let form = [
            ("task", "20"),
            ("folder_id", &target_folder_id), // The destination
            ("file_id", &item_id),            // The item being moved
        ];

        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        Ok(json["zt"] == 1)
    }

    pub async fn restore_item(
        &self,
        id: &str,
        is_folder: bool,
        formhash: &str,
    ) -> Result<bool, String> {
        let url = format!("{}/mydisk.php?item=recycle", BASE_URL);
        let action = if is_folder {
            "folder_restore"
        } else {
            "file_restore"
        };
        let id_key = if is_folder { "folder_id" } else { "file_id" };

        let form = [
            ("action", action),
            ("task", action),
            (id_key, id),
            (
                "ref",
                "https://up.woozooo.com/mydisk.php?item=recycle&action=files",
            ),
            ("formhash", formhash),
        ];

        let resp = self
            .client
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let html = resp.text().await.map_err(|e| e.to_string())?;
        Ok(html.contains("恢复成功"))
    }

    pub async fn hard_delete_item(
        &self,
        id: &str,
        is_folder: bool,
        formhash: &str,
    ) -> Result<bool, String> {
        let url = format!("{}/mydisk.php?item=recycle", BASE_URL);
        let action = if is_folder {
            "folder_delete_complete"
        } else {
            "file_delete_complete"
        };
        let id_key = if is_folder { "folder_id" } else { "file_id" };

        let form = [
            ("action", action),
            ("task", action),
            (id_key, id),
            (
                "ref",
                "https://up.woozooo.com/mydisk.php?item=recycle&action=files",
            ),
            ("formhash", formhash),
        ];

        let resp = self
            .client
            .post(&url)
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let html = resp.text().await.map_err(|e| e.to_string())?;
        Ok(html.contains("删除成功"))
    }

    pub async fn create_folder_in_target(
        &self,
        folder_name: String,
        folder_description: String,
        parent_id: String,
    ) -> Result<Value, String> {
        let url = format!("{}/doupload.php", BASE_URL);

        let form = [
            ("task", "2"),
            ("parent_id", &parent_id),
            ("folder_name", &folder_name),
            ("folder_description", &folder_description),
        ];

        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        if json["zt"] == 1 {
            Ok(json)
        } else {
            Err("Failed to create target folder".to_string())
        }
    }

    pub async fn upload_file_direct(
        &self,
        bytes: bytes::Bytes,
        safe_name: String,
        target_folder: String,
        task_id: String,
        offset_base: usize,
        total_file_size: usize,
        task_flag: Arc<AtomicU8>,
        upload_limit: Arc<std::sync::atomic::AtomicU32>,
        file_index: usize,
    ) -> Result<String, String> {
        let mime = mime_guess::from_path(&safe_name)
            .first_or_octet_stream()
            .to_string();
        let parent_id = if target_folder == "-1" { "0".to_string() } else { target_folder };

        let total_bytes = bytes.len();
        let chunk_size = 256 * 1024;

        // Use a dedicated stream-abortion flag.
        let stalled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shared_offset = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let stall_clone = stalled.clone();
        let offset_watchdog = shared_offset.clone();
        let task_flag_watchdog = task_flag.clone();
        let limit_watchdog = upload_limit.clone();
        let total_bytes_watchdog = total_bytes;

        let watchdog_handle = tokio::spawn(async move {
            let mut last_offset = 0;
            let mut server_wait_ticks = 0;

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if task_flag_watchdog.load(Ordering::SeqCst) != 0 { break; } 
                
                let current_offset = offset_watchdog.load(Ordering::SeqCst);
                
                // Phase 2: If data is 100% sent, Lanzou often hangs while combining the file.
                if current_offset >= total_bytes_watchdog {
                    server_wait_ticks += 1;
                    if server_wait_ticks > 6 { // 30 seconds max wait for Lanzou's HTTP 200 OK
                        println!("[WATCHDOG] Lanzou server hung processing the file. Force-aborting...");
                        stall_clone.store(true, Ordering::SeqCst);
                        break;
                    }
                    continue;
                }

                let bytes_sent = current_offset.saturating_sub(last_offset);
                
                let mut min_expected = 750_000;
                
                // Scale down expectation if user explicitly set a slower manual limit
                let user_limit_kb = limit_watchdog.load(Ordering::Relaxed);
                if user_limit_kb > 0 {
                    min_expected = std::cmp::min(min_expected, (user_limit_kb as usize * 1024 * 5) / 2);
                }

                if bytes_sent < min_expected {
                    println!("[WATCHDOG] Speed dropped to dead levels ({} B/5s). Triggering auto-retry...", bytes_sent);
                    stall_clone.store(true, Ordering::SeqCst);
                    break;
                }
                last_offset = current_offset;
            }
        });

        let task_flag_stream = task_flag.clone();
        let stream = async_stream::stream! {
            let mut offset = 0;
            let mut start_time = tokio::time::Instant::now();

            while offset < total_bytes {
                let state = task_flag_stream.load(Ordering::SeqCst);
                if state == 1 { yield Err::<bytes::Bytes, std::io::Error>(std::io::Error::new(std::io::ErrorKind::Interrupted, "PAUSED")); break; }
                if state == 2 { yield Err::<bytes::Bytes, std::io::Error>(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "CANCELLED")); break; }
                
                // IF the watchdog detected a stall or low speed, yield an error to kill Reqwest!
                if stalled.load(Ordering::SeqCst) {
                    yield Err::<bytes::Bytes, std::io::Error>(std::io::Error::new(std::io::ErrorKind::TimedOut, "NETWORK_STALLED")); 
                    break; 
                }

                let end = std::cmp::min(offset + chunk_size, total_bytes);
                let chunk = bytes.slice(offset..end);

                let limit_kb = upload_limit.load(Ordering::Relaxed);
                if limit_kb > 0 {
                    let expected_time = std::time::Duration::from_secs_f64((end - offset) as f64 / (limit_kb * 1024) as f64);
                    let elapsed = start_time.elapsed();
                    if elapsed < expected_time {
                        tokio::time::sleep(expected_time - elapsed).await;
                    }
                }
                start_time = tokio::time::Instant::now();
                offset = end;

                // Sync exact progress to the watchdog
                shared_offset.store(offset, Ordering::SeqCst);

                yield Ok::<bytes::Bytes, std::io::Error>(chunk);
            }
        };

        let body = reqwest::Body::wrap_stream(stream);
        let part = multipart::Part::stream_with_length(body, total_bytes as u64)
            .file_name(safe_name.clone())
            .mime_str(&mime)
            .unwrap();

        let wu_id = format!("WU_FILE_{}", file_index);
        let form = multipart::Form::new()
            .text("task", "1")
            .text("vie", "2")
            .text("ve", "2")
            .text("id", wu_id)
            .text("folder_id_bb_n", parent_id)
            .text("name", safe_name)
            .text("type", mime.clone())
            .text("size", total_bytes.to_string())
            .part("upload_file", part);

        let url = format!("{}/html5up.php", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", "https://up.woozooo.com")
            .header("Referer", "https://up.woozooo.com/mydisk.php")
            .header("Accept-Language", "en-US,en;q=0.9,zh-CN;q=0.8")
            .multipart(form)
            .send()
            .await;

        watchdog_handle.abort();

        let resp = resp.map_err(|e| e.to_string())?;
        let json: Value = resp.json().await.map_err(|e| e.to_string())?;
        
        if json["zt"] == 1 {
            let file_id = json["text"][0]["id"].as_str().unwrap_or("").to_string();
            Ok(file_id)
        } else {
            Err(format!("Upload failed: {}", json))
        }
    }

    pub async fn login(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<(String, String), String> {
        let base_url = "https://accounts.woozooo.com/accounts.php";
        let referer_url = format!("{}?action=login&ref=up.woozooo.com", base_url);

        // --- STEP 1: Trigger the WAF Challenge ---
        let get_resp = self
            .client
            .get(&referer_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let body_text = get_resp.text().await.unwrap_or_default();
        let mut acw_sc_cookie = String::new();

        // --- STEP 2: Pure Rust WAF Bypass ---
        if body_text.contains("arg1=") {
            println!("\n[INFO] Ali WAF challenge detected. Solving natively in Rust...");

            let re = Regex::new(r"arg1='([A-F0-9]+)'").unwrap();
            if let Some(caps) = re.captures(&body_text) {
                let arg1 = &caps[1];
                let generated_cookie = solve_ali_waf(arg1);

                println!("[SUCCESS] WAF Cookie Generated: {}", generated_cookie);
                acw_sc_cookie = format!("acw_sc__v2={}", generated_cookie);
            } else {
                return Err("WAF challenge detected, but could not extract arg1.".to_string());
            }
        }

        // --- STEP 3: The Actual Login POST ---
        let form = [
            ("task", "uselogin"),
            ("username", username),
            ("password", password),
            ("ref", "up.woozooo.com"),
        ];

        let mut post_req = self
            .client
            .post(base_url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            )
            .header("Accept", "application/json, text/javascript, */*")
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Origin", "https://accounts.woozooo.com")
            .header("Host", "accounts.woozooo.com")
            .header("Referer", &referer_url);

        // Inject the natively generated clearance cookie
        if !acw_sc_cookie.is_empty() {
            post_req = post_req.header("Cookie", acw_sc_cookie);
        }

        let resp = post_req
            .form(&form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let mut ylogin = String::new();
        let mut phpdisk_info = String::new();

        for cookie in resp.headers().get_all(reqwest::header::SET_COOKIE) {
            if let Ok(c_str) = cookie.to_str() {
                if c_str.starts_with("ylogin=") {
                    ylogin = c_str.split(';').next().unwrap().replace("ylogin=", "");
                }
                if c_str.starts_with("phpdisk_info=") {
                    phpdisk_info = c_str
                        .split(';')
                        .next()
                        .unwrap()
                        .replace("phpdisk_info=", "");
                }
            }
        }

        let final_body = resp.text().await.unwrap_or_default();

        if ylogin.is_empty() || phpdisk_info.is_empty() {
            println!("[ERROR] Final login body: {}", final_body);
            return Err(format!("Login failed. Server response: {}", final_body));
        }

        self.set_cookies(ylogin.clone(), phpdisk_info.clone());
        Ok((ylogin, phpdisk_info))
    }

    pub async fn request_register_sms(&self, phone: &str) -> Result<String, String> {
        let form = [("task", "register"), ("phone", phone)];
        let json = self.post_with_waf(&form).await?;

        if json["zt"] == 1 {
            Ok(json["msgs"].as_str().unwrap_or("SMS sent").to_string())
        } else {
            Err(json["msgs"]
                .as_str()
                .unwrap_or("Failed to send SMS")
                .to_string())
        }
    }

    pub async fn submit_register(
        &self,
        phone: &str,
        code: &str,
        password: &str,
    ) -> Result<bool, String> {
        // Step 1: Verify the SMS code
        let code_form = [
            ("task", "update_code"),
            ("phone", phone),
            ("verycode", code),
        ];
        let code_json = self.post_with_waf(&code_form).await?;

        if code_json["zt"] != 1 {
            return Err("Invalid verification code".to_string());
        }

        // Step 2: Set the password
        let pwd_form = [
            ("task", "update_pwd"),
            ("phone", phone),
            ("verycode", code),
            ("password1", password),
            ("password2", password),
        ];
        let pwd_json = self.post_with_waf(&pwd_form).await?;

        if pwd_json["zt"] == 1 {
            Ok(true)
        } else {
            Err("Failed to set password".to_string())
        }
    }
}

// --------------------------------------------------------
// Tauri Commands
// --------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub lanzou: Arc<tokio::sync::Mutex<LanzouCloud>>,
    pub downloader: Arc<tokio::sync::Mutex<crate::openwrt_lanzou_down::LanzouDownloader>>,
    pub vfs: Arc<tokio::sync::Mutex<Option<VfsTree>>>,
    pub pid_stack: Arc<tokio::sync::Mutex<Vec<u64>>>,
    pub task_ctrl: Arc<
        tokio::sync::Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicU8>>>,
    >,
    pub sync_lock: Arc<tokio::sync::Mutex<()>>,
    pub upload_limit: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub download_limit: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub current_phone: Arc<tokio::sync::Mutex<String>>,
}

#[derive(serde::Serialize, Clone)]
struct ProgressPayload {
    task_id: String,
    loaded: usize,
    total: usize,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DropFile {
    pub path: String,
    pub target_pid: u64,
    pub group_id: Option<String>,
    pub group_name: Option<String>,
}

#[derive(serde::Serialize)]
pub struct Breadcrumb {
    pub id: u64,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct FlatFile {
    pub id: u64,
    pub name: String,
    pub size: String,
    pub rel_path: String,
}

#[derive(Serialize, Deserialize)]
pub struct SharePayload {
    pub n: String, // name
    pub m: String, // md5
    pub s: String, // size
    pub c: u32,    // chunks
    pub l: String, // lanzou_share_url (e.g. "lanzoux.com/xxxx")
    pub p: String, // password
}

#[derive(Serialize)]
pub struct ResolveResult {
    pub name: String,
    pub size: String,
    pub md5: String,
    pub chunks: u32,
    pub is_folder: bool,
}

fn rebuild_folder_recursive<'a>(
    tree: &'a mut VfsTree,
    lanzou: &'a LanzouCloud,
    node_id: u64,
    new_parent_pid: u64,
    new_parent_lanzou_id: String,
) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        // 1. Get the current node's info
        let node = tree.nodes.get(&node_id).cloned().ok_or("Node not found")?;

        let mut depth = 0;
        let mut curr = new_parent_pid;
        while curr != 0 {
            if let Some(n) = tree.nodes.get(&curr) {
                depth += 1;
                curr = n.pid;
            } else {
                break;
            }
        }

        // Override the passed-in parent if we crossed the threshold
        let actual_target_lanzou_id = if depth >= 2 {
            tree.deeperdir_lanzou_id.clone()
        } else {
            new_parent_lanzou_id.clone()
        };

        if node.node_type == NodeType::Directory {
            // It's a folder. We must create a clone of it in the new destination.
            let res = lanzou
                .create_folder_in_target(
                    node.name.clone(),
                    "".to_string(),
                    actual_target_lanzou_id.clone(),
                )
                .await?;

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let cloned_lanzou_id = res["text"].as_str().unwrap_or("").to_string();
            if cloned_lanzou_id.is_empty() {
                return Err("Failed to create rebuilt folder on cloud".to_string());
            }

            // Find all children BEFORE we modify the parent
            let children: Vec<u64> = tree
                .nodes
                .values()
                .filter(|n| n.pid == node_id)
                .map(|n| n.id)
                .collect();

            // Recursively move all children into the newly cloned folder
            for child_id in children {
                rebuild_folder_recursive(tree, lanzou, child_id, node_id, cloned_lanzou_id.clone())
                    .await?;
            }

            // Delete the old folder from Lanzou
            let _ = lanzou.delete_folder(node.lanzou_id.clone()).await;

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            // Update the VFS tree to point to the new Lanzou ID and Parent PID
            if let Some(mut_node) = tree.nodes.get_mut(&node_id) {
                mut_node.pid = new_parent_pid;
                mut_node.lanzou_id = cloned_lanzou_id;
                mut_node.time = crate::openwrt_heriheri::current_timestamp();
            }
        } else {
            if node.lanzou_id.starts_with("alien://") {
            } else if node.chunks != "1" && !node.chunks.is_empty() {
                let res = lanzou
                    .create_folder_in_target(
                        node.md5.clone(),
                        "".to_string(),
                        actual_target_lanzou_id.clone(), // <-- Use actual_target
                    )
                    .await?;

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                let new_chunk_folder_id = res["text"].as_str().unwrap_or("").to_string();

                if !new_chunk_folder_id.is_empty() {
                    if let Ok(parts) = lanzou.list_files_by_id(&node.lanzou_id).await {
                        for part in parts {
                            if let Some(fid) = part["id"].as_str() {
                                let _ = lanzou
                                    .move_item(fid.to_string(), new_chunk_folder_id.clone())
                                    .await;

                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            }
                        }
                    }
                    let _ = lanzou.delete_folder(node.lanzou_id.clone()).await;

                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                    if let Some(mut_node) = tree.nodes.get_mut(&node_id) {
                        mut_node.lanzou_id = new_chunk_folder_id;
                    }
                }
            } else {
                // --- Standard Single File Move ---
                let _ = lanzou
                    .move_item(node.lanzou_id.clone(), actual_target_lanzou_id) // <-- Use actual_target
                    .await;

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            // Update the VFS tree
            if let Some(mut_node) = tree.nodes.get_mut(&node_id) {
                mut_node.pid = new_parent_pid;
                mut_node.time = crate::openwrt_heriheri::current_timestamp();
            }
        }

        Ok(())
    })
}

fn get_descendants(tree: &crate::openwrt_heriheri::VfsTree, ids: &[u64], top_down: bool) -> Vec<u64> {
    let mut result = Vec::new();
    let mut queue = ids.to_vec();
    let mut seen = std::collections::HashSet::new();

    while let Some(current) = queue.pop() {
        if seen.insert(current) {
            result.push(current);
            // Find all immediate children and queue them up
            for node in tree.nodes.values() {
                if node.pid == current {
                    queue.push(node.id);
                }
            }
        }
    }

    // Depth-First Search naturally yields a Top-Down list.
    // Reversing it gives us Bottom-Up (Deepest First).
    if !top_down {
        result.reverse();
    }
    result
}

fn get_files_recursively(dir: &std::path::Path, files: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                files.push(path.to_string_lossy().to_string());
            } else if path.is_dir() {
                get_files_recursively(&path, files);
            }
        }
    }
}

pub async fn internal_create_folder(
    name: &str,
    target_pid: u64,
    state: &AppState,
) -> Result<u64, String> {
    let lanzou = state.lanzou.lock().await;
    let mut vfs_guard = state.vfs.lock().await;

    if let Some(tree) = vfs_guard.as_mut() {
        let mut depth = 0;
        let mut curr = target_pid;
        while curr != 0 {
            if let Some(n) = tree.nodes.get(&curr) {
                depth += 1;
                curr = n.pid;
            } else {
                break;
            }
        }

        let target_lanzou_folder = if depth >= 2 {
            tree.deeperdir_lanzou_id.clone() // Flatten deep folders
        } else if target_pid == 0 {
            tree.root_lanzou_id.clone()
        } else {
            tree.nodes
                .get(&target_pid)
                .map(|n| n.lanzou_id.clone())
                .unwrap_or(tree.root_lanzou_id.clone())
        };

        let res = lanzou
            .create_folder_in_target(name.to_string(), "".to_string(), target_lanzou_folder)
            .await?;
        let new_lanzou_id = res["text"].as_str().unwrap_or("").to_string();

        let new_pid = tree.create_folder(target_pid, name, &new_lanzou_id);
        tree.save_local()?;
        Ok(new_pid)
    } else {
        Err("VFS Offline".to_string())
    }
}

fn flatten_tree(tree: &VfsTree, pid: u64, current_path: String, out: &mut Vec<FlatFile>) {
    let children: Vec<_> = tree
        .nodes
        .values()
        .filter(|n| n.pid == pid)
        .cloned()
        .collect();
    for node in children {
        let node_path = if current_path.is_empty() {
            node.name.clone()
        } else {
            format!("{}/{}", current_path, node.name)
        };
        let is_chunked =
            node.node_type == NodeType::File && node.chunks != "1" && !node.chunks.is_empty();

        if node.node_type == NodeType::File || is_chunked {
            out.push(FlatFile {
                id: node.id,
                name: node.name,
                size: node.size,
                rel_path: node_path,
            });
        } else if node.node_type == NodeType::Directory {
            flatten_tree(tree, node.id, node_path, out);
        }
    }
}

async fn get_sync_folder_id(lanzou: &LanzouCloud) -> Result<String, String> {
    let root_id = "-1";

    let folders = lanzou.list_folders_by_id(root_id).await?;
    for f in folders {
        if f["name"].as_str().unwrap_or("") == ".vfs" {
            let id = f["fol_id"].as_str().unwrap_or("").to_string();
            if !id.is_empty() {
                return Ok(id);
            }
            if let Some(n) = f["fol_id"].as_u64() {
                return Ok(n.to_string());
            }
        }
    }

    let res = lanzou
        .create_folder_in_target(
            ".vfs".to_string(),
            "HeriHeri Sync Data".to_string(),
            root_id.to_string(),
        )
        .await?;
    let new_id = res["text"].as_str().unwrap_or("").to_string();
    if !new_id.is_empty() {
        return Ok(new_id);
    }
    if let Some(n) = res["text"].as_u64() {
        return Ok(n.to_string());
    }

    Err("Failed to initialize .vfs sync folder".to_string())
}

pub async fn execute_sync_pull(state: &AppState) -> Result<bool, String> {
    let (lanzou, downloader, local_timestamp, local_path) = {
        let lanzou_guard = state.lanzou.lock().await;
        let down_guard = state.downloader.lock().await;
        let vfs_guard = state.vfs.lock().await;

        let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;
        (
            lanzou_guard.clone(),
            down_guard.clone(),
            tree.last_modified,
            tree.file_path.clone(),
        )
    };

    let sync_folder_id = get_sync_folder_id(&lanzou).await?;
    let files = lanzou.list_files_by_id(&sync_folder_id).await?;

    let mut highest_cloud_ts: u64 = 0;
    let mut target_file_id = String::new();
    let phone = state.current_phone.lock().await.clone();
    let re_pattern = if phone.is_empty() {
        r"heriheri_tree_(\d+)\.txt".to_string()
    } else {
        format!(r"heriheri_tree_{}_(\d+)\.txt", phone)
    };
    let re = regex::Regex::new(&re_pattern).unwrap();

    for f in files {
        if let Some(name) = f["name"].as_str() {
            if let Some(caps) = re.captures(name) {
                let ts = caps[1].parse::<u64>().unwrap_or(0);
                if ts > highest_cloud_ts {
                    highest_cloud_ts = ts;
                    target_file_id = f["id"].as_str().unwrap_or("").to_string();
                }
            }
        }
    }

    if highest_cloud_ts <= local_timestamp || target_file_id.is_empty() {
        return Ok(false);
    }

    println!(
        "[SYNC] Cloud is newer ({} > {}). Pulling state...",
        highest_cloud_ts, local_timestamp
    );

    let share_info = lanzou.get_share_info(target_file_id.clone(), false).await?;
    let share_url = if let Some(u) = share_info["new_url"].as_str() {
        u.to_string()
    } else {
        format!(
            "{}/{}",
            share_info["is_newd"].as_str().unwrap_or(""),
            share_info["f_id"].as_str().unwrap_or("")
        )
    };

    let direct_url = downloader.get_lanzou_direct_link(&share_url, None).await?;

    let req_client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()
        .unwrap();
    let resp = req_client
        .get(&direct_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err("Failed to download cloud TSV".to_string());
    }
    let cloud_tsv_content = resp.text().await.map_err(|e| e.to_string())?;

    let cloud_tree = VfsTree::from_tsv(&cloud_tsv_content, local_path.clone())?;

    let mut vfs_guard = state.vfs.lock().await;
    if let Some(local_tree) = vfs_guard.as_mut() {
        let merged_tree = local_tree.merge_with(&cloud_tree);
        merged_tree.save_local()?;
        *local_tree = merged_tree;
    }

    Ok(true)
}