use crate::heriheri::{get_safe_lanzou_ext, NodeType, VfsNode, VfsTree};
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
use tauri::Emitter;
use tauri::{AppHandle, Manager, State};
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
        bytes: Vec<u8>,
        safe_name: String,
        target_folder: String,
        app: tauri::AppHandle,
        task_id: String,
        offset_base: usize,
        total_file_size: usize,
        task_flag: Arc<AtomicU8>,
        upload_limit: Arc<std::sync::atomic::AtomicU32>,
    ) -> Result<String, String> {
        let mime = mime_guess::from_path(&safe_name)
            .first_or_octet_stream()
            .to_string();
        let parent_id = if target_folder == "-1" {
            "0".to_string()
        } else {
            target_folder
        };

        let total_bytes = bytes.len();
        let chunk_size = 256 * 1024;

        let stream = async_stream::stream! {
            let mut offset = 0;
            let mut start_time = tokio::time::Instant::now();

            while offset < total_bytes {
                let state = task_flag.load(Ordering::SeqCst);
                if state == 1 { yield Err::<Vec<u8>, std::io::Error>(std::io::Error::new(std::io::ErrorKind::Interrupted, "PAUSED")); break; }
                if state == 2 { yield Err::<Vec<u8>, std::io::Error>(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "CANCELLED")); break; }

                let end = std::cmp::min(offset + chunk_size, total_bytes);
                let chunk = bytes[offset..end].to_vec();

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

                // Emit real-time byte-level progress to React
                let _ = app.emit("upload_progress", ProgressPayload {
                    task_id: task_id.clone(),
                    loaded: offset_base + offset,
                    total: total_file_size,
                });

                // MINIMUM FIX: Explicitly typed Ok without the ambiguous .into()
                yield Ok::<Vec<u8>, std::io::Error>(chunk);
            }
        };

        // Wrap the stream into a Reqwest body
        let body = reqwest::Body::wrap_stream(stream);
        let part = multipart::Part::stream_with_length(body, total_bytes as u64)
            .file_name(safe_name.clone())
            .mime_str(&mime)
            .unwrap();

        let form = multipart::Form::new()
            .text("task", "1")
            .text("vie", "2")
            .text("ve", "2")
            .text("id", "WU_FILE_0")
            .text("folder_id_bb_n", parent_id)
            .text("name", safe_name)
            .part("upload_file", part);

        let url = format!("{}/html5up.php", BASE_URL);
        let resp = self
            .client
            .post(&url)
            .header("X-Requested-With", "XMLHttpRequest")
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;

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
    pub downloader: Arc<tokio::sync::Mutex<crate::lanzou_down::LanzouDownloader>>,
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
                mut_node.time = crate::heriheri::current_timestamp();
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
                mut_node.time = crate::heriheri::current_timestamp();
            }
        }

        Ok(())
    })
}

fn get_descendants(tree: &crate::heriheri::VfsTree, ids: &[u64], top_down: bool) -> Vec<u64> {
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

#[tauri::command]
pub async fn vfs_control_task(
    task_id: String,
    action: u8,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let map = state.task_ctrl.lock().await;
    if let Some(flag) = map.get(&task_id) {
        flag.store(action, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_lanzou_cookies(
    ylogin: String,
    phpdisk_info: String,
    phone: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let mut lanzou = state.lanzou.lock().await;
    lanzou.set_cookies(ylogin, phpdisk_info);
    *state.current_phone.lock().await = phone;
    lanzou.is_logged_in().await
}

#[tauri::command]
pub async fn init_vfs_root(
    phone: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let _ = crate::heriheri::current_timestamp();
    
    {
        let mut current_phone = state.current_phone.lock().await;
        *current_phone = phone.clone();
    }

    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;

    let file_name = if phone.is_empty() {
        "heriheri_tree.txt".to_string()
    } else {
        format!("heriheri_tree_{}.txt", phone)
    };
    let tree_path = app_data_dir.join(file_name);

    let mut lanzou = state.lanzou.lock().await;
    let (root_lanzou_id, deeperdir_lanzou_id) = lanzou.init_vfs_root().await?;

    let mut vfs_guard = state.vfs.lock().await;

    if let Ok(mut tree) = VfsTree::load_local(tree_path.clone()) {
        if tree.deeperdir_lanzou_id.is_empty() {
            tree.deeperdir_lanzou_id = deeperdir_lanzou_id;
            let _ = tree.save_local();
        }
        *vfs_guard = Some(tree);
    } else {
        let mut tree = VfsTree::new(root_lanzou_id, deeperdir_lanzou_id, tree_path);
        tree.save_local()?;
        *vfs_guard = Some(tree);
    }

    let mut stack = state.pid_stack.lock().await;
    *stack = vec![0];
    Ok(())
}

#[tauri::command]
pub async fn vfs_list_dir(state: State<'_, AppState>) -> Result<Vec<VfsNode>, String> {
    let vfs_guard = state.vfs.lock().await;
    let stack = state.pid_stack.lock().await;
    let current_pid = *stack.last().unwrap_or(&0);

    if let Some(tree) = &*vfs_guard {
        let nodes = tree
            .list_dir(current_pid)
            .into_iter()
            .filter(|n| !n.is_trashed)
            .collect();
        Ok(nodes)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_enter_folder(id: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut stack = state.pid_stack.lock().await;
    stack.push(id);
    Ok(())
}

#[tauri::command]
pub async fn vfs_go_back(state: State<'_, AppState>) -> Result<(), String> {
    let mut stack = state.pid_stack.lock().await;
    if stack.len() > 1 {
        stack.pop();
    }
    Ok(())
}

#[tauri::command]
pub async fn vfs_create_folder(
    name: String,
    desc: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let lanzou = state.lanzou.lock().await;
    let mut vfs_guard = state.vfs.lock().await;
    let stack = state.pid_stack.lock().await;
    let current_pid = *stack.last().unwrap_or(&0);

    if let Some(tree) = vfs_guard.as_mut() {
        let mut depth = 0;
        let mut curr = current_pid;
        while curr != 0 {
            if let Some(n) = tree.nodes.get(&curr) {
                depth += 1;
                curr = n.pid;
            } else {
                break;
            }
        }

        // Resolve the physical destination
        let target_lanzou_folder = if depth >= 2 {
            tree.deeperdir_lanzou_id.clone() // Flatten deep folders
        } else if current_pid == 0 {
            tree.root_lanzou_id.clone()
        } else {
            tree.nodes
                .get(&current_pid)
                .map(|n| n.lanzou_id.clone())
                .unwrap_or(tree.root_lanzou_id.clone())
        };

        let res = lanzou
            .create_folder_in_target(name.clone(), desc, target_lanzou_folder)
            .await?;
        let new_lanzou_id = res["text"].as_str().unwrap_or("").to_string();

        tree.create_folder(current_pid, &name, &new_lanzou_id);
        tree.save_local()?;
        Ok(())
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_upload_file(
    file_path: String,
    task_id: String,
    target_pid: u64,
    resume_folder: String,
    resume_chunk: usize,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let task_flag = Arc::new(AtomicU8::new(0));
    state
        .task_ctrl
        .lock()
        .await
        .insert(task_id.clone(), task_flag.clone());

    let (lanzou_clone, target_lanzou_folder) = {
        let lanzou = state.lanzou.lock().await;
        let vfs_guard = state.vfs.lock().await;

        let target_folder = if let Some(tree) = &*vfs_guard {
            if target_pid == 0 {
                tree.root_lanzou_id.clone()
            } else {
                tree.nodes
                    .get(&target_pid)
                    .map(|n| n.lanzou_id.clone())
                    .unwrap_or(tree.root_lanzou_id.clone())
            }
        } else {
            return Err("VFS Offline".to_string());
        };

        if target_folder.is_empty() {
            return Err("VFS Offline".to_string());
        }
        (lanzou.clone(), target_folder)
    };

    let path = std::path::PathBuf::from(&file_path);
    let original_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let original_ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let safe_ext = crate::heriheri::get_safe_lanzou_ext(&original_ext);

    let (bytes, md5_str, total_size, size_str) = tokio::task::spawn_blocking(move || {
        let b = std::fs::read(&path).map_err(|e| e.to_string())?;
        let t = b.len();
        let s = t.to_string();
        let h = md5::compute(&b);
        let m = format!("{:x}", h);
        Ok::<_, String>((b, m, t, s))
    })
    .await
    .unwrap()?;

    let mut instant_copy = None;
    {
        let vfs_guard = state.vfs.lock().await;
        if let Some(tree) = &*vfs_guard {
            if let Some(existing_node) = tree.nodes.values().find(|n| {
                n.md5 == md5_str
                    && n.node_type == NodeType::File
                    && !n.is_deleted
                    && !n.is_trashed
                    && !n.lanzou_id.starts_with("alien://")
            }) {
                instant_copy = Some((
                    existing_node.lanzou_id.clone(),
                    existing_node.chunks.clone(),
                ));
            }
        }
    }

    if let Some((existing_lanzou_id, existing_chunks)) = instant_copy {
        let mut vfs_guard = state.vfs.lock().await;
        if let Some(tree) = vfs_guard.as_mut() {
            let chunks_u32 = existing_chunks.parse::<u32>().unwrap_or(1);
            let alien_nodes_to_remove: Vec<u64> = tree
                .nodes
                .values()
                .filter(|n| {
                    n.pid == target_pid && n.md5 == md5_str && n.lanzou_id.starts_with("alien://")
                })
                .map(|n| n.id)
                .collect();
            for id in alien_nodes_to_remove {
                tree.delete_node(id);
            }
            tree.add_file(
                target_pid,
                &original_name,
                &existing_lanzou_id,
                &size_str,
                &md5_str,
                &original_ext,
                chunks_u32,
            );
            tree.save_local()?;
        }
        state.task_ctrl.lock().await.remove(&task_id);
        return Ok(()); // Instantly resolves as "Success" without uploading a single byte!
    }

    let chunk_limit = 100 * 1024 * 1024;
    let _ = app.emit(
        "upload_progress",
        ProgressPayload {
            task_id: task_id.clone(),
            loaded: resume_chunk * chunk_limit,
            total: total_size,
        },
    );

    let final_lanzou_id;
    let final_chunks;

    if total_size <= chunk_limit {
        let safe_name = format!("{}.{}", md5_str, safe_ext);
        let res = lanzou_clone
            .upload_file_direct(
                bytes,
                safe_name,
                target_lanzou_folder,
                app.clone(),
                task_id.clone(),
                0,
                total_size,
                task_flag.clone(),
                state.upload_limit.clone(),
            )
            .await;

        if let Err(e) = res {
            if e.contains("CANCELLED") {
                return Err("CANCELLED".to_string());
            }
            return Err(e);
        }
        final_lanzou_id = res.unwrap();
        final_chunks = 1;
    } else {
        final_lanzou_id = if resume_folder.is_empty() {
            let res = lanzou_clone
                .create_folder_in_target(md5_str.clone(), "".to_string(), target_lanzou_folder)
                .await?;
            res["text"].as_str().unwrap_or("").to_string()
        } else {
            resume_folder.clone()
        };

        if final_lanzou_id.is_empty() {
            return Err("Failed to create chunk directory on cloud".to_string());
        }

        let num_chunks = (total_size + chunk_limit - 1) / chunk_limit;
        final_chunks = num_chunks as u32;
        let mut current_loaded = resume_chunk * chunk_limit;

        for i in resume_chunk..num_chunks {
            let start = i * chunk_limit;
            let end = std::cmp::min(start + chunk_limit, total_size);

            let chunk_bytes = bytes[start..end].to_vec();
            let chunk_name = format!("{}_part{}.iso", md5_str, i + 1);

            let upload_res = lanzou_clone
                .upload_file_direct(
                    chunk_bytes,
                    chunk_name,
                    final_lanzou_id.clone(),
                    app.clone(),
                    task_id.clone(),
                    current_loaded,
                    total_size,
                    task_flag.clone(),
                    state.upload_limit.clone(),
                )
                .await;

            if let Err(e) = upload_res {
                if e.contains("PAUSED") {
                    return Err(format!("PAUSED:{}|{}", final_lanzou_id, i));
                }
                if e.contains("CANCELLED") {
                    lanzou_clone.delete_folder(final_lanzou_id).await?;
                    return Err("CANCELLED".to_string());
                }
                return Err(e);
            }
            current_loaded += end - start;
        }
    }

    state.task_ctrl.lock().await.remove(&task_id);

    let mut vfs_guard = state.vfs.lock().await;
    if let Some(tree) = vfs_guard.as_mut() {
        let alien_nodes_to_remove: Vec<u64> = tree
            .nodes
            .values()
            .filter(|n| {
                n.pid == target_pid && n.md5 == md5_str && n.lanzou_id.starts_with("alien://")
            })
            .map(|n| n.id)
            .collect();
        for id in alien_nodes_to_remove {
            tree.delete_node(id);
        }
        tree.add_file(
            target_pid,
            &original_name,
            &final_lanzou_id,
            &size_str,
            &md5_str,
            &original_ext,
            final_chunks,
        );
        tree.save_local()?;
    }

    Ok(())
}

#[tauri::command]
pub async fn vfs_delete_item(id: u64, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let lanzou = state.lanzou.lock().await;
    let mut vfs_guard = state.vfs.lock().await;

    if let Some(tree) = vfs_guard.as_mut() {
        let (lanzou_id, is_physical_folder) = {
            let node = tree.nodes.get(&id).ok_or("Node not found in VFS")?;
            let is_dir = node.node_type == crate::heriheri::NodeType::Directory;
            let is_chunked = node.node_type == crate::heriheri::NodeType::File
                && node.chunks != "1"
                && !node.chunks.is_empty();
            (node.lanzou_id.clone(), is_dir || is_chunked)
        };

        if is_physical_folder {
            let _ = lanzou.delete_folder(lanzou_id).await;
        } else {
            let _ = lanzou.delete_file(lanzou_id).await;
        }

        tree.delete_node(id);

        tree.save_local()?;
        Ok(true)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_get_share_info(id: u64, state: State<'_, AppState>) -> Result<Value, String> {
    let lanzou = state.lanzou.lock().await;
    let vfs_guard = state.vfs.lock().await;

    if let Some(tree) = &*vfs_guard {
        let node = tree.nodes.get(&id).ok_or("Node not found in VFS")?;

        // KEY FIX: Sharing a chunked file means we are sharing its physical parent folder
        let is_physical_folder = node.node_type == NodeType::Directory
            || (node.node_type == NodeType::File && node.chunks != "1" && !node.chunks.is_empty());

        lanzou
            .get_share_info(node.lanzou_id.clone(), is_physical_folder)
            .await
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn login(
    username: String,
    password: String,
    state: State<'_, AppState>,
) -> Result<(String, String), String> {
    let mut lanzou = state.lanzou.lock().await;
    lanzou.login(&username, &password).await
}

#[tauri::command]
pub async fn request_register_sms(
    phone: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let lanzou = state.lanzou.lock().await;
    lanzou.request_register_sms(&phone).await
}

#[tauri::command]
pub async fn submit_register(
    phone: String,
    code: String,
    password: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let lanzou = state.lanzou.lock().await;
    lanzou.submit_register(&phone, &code, &password).await
}

#[tauri::command]
pub async fn vfs_get_current_pid(state: State<'_, AppState>) -> Result<u64, String> {
    let stack = state.pid_stack.lock().await;
    Ok(*stack.last().unwrap_or(&0))
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

async fn internal_create_folder(
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

#[tauri::command]
pub async fn vfs_expand_drop(
    paths: Vec<String>,
    current_pid: u64,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DropFile>, String> {
    let mut result = Vec::new();

    for path_str in paths {
        let path = std::path::Path::new(&path_str);
        if path.is_file() {
            result.push(DropFile {
                path: path_str,
                target_pid: current_pid,
                group_id: None,
                group_name: None,
            });
        } else if path.is_dir() {
            let folder_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let folder_pid = match internal_create_folder(&folder_name, current_pid, &state).await {
                Ok(pid) => pid,
                Err(_) => current_pid,
            };

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

            let group_id = Some(format!(
                "g_{}_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
                folder_pid
            ));
            let group_name = Some(folder_name.clone());

            // Iteratively process all nested directories
            let mut dirs_to_process = vec![(path.to_path_buf(), folder_pid)];
            while let Some((current_dir, pid)) = dirs_to_process.pop() {
                if let Ok(entries) = std::fs::read_dir(&current_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() || p.is_symlink() {
                            result.push(DropFile {
                                path: p.to_string_lossy().to_string(),
                                target_pid: pid,
                                group_id: group_id.clone(),
                                group_name: group_name.clone(),
                            });
                        } else if p.is_dir() {
                            let child_name = p
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let child_pid =
                                match internal_create_folder(&child_name, pid, &state).await {
                                    Ok(id) => id,
                                    Err(_) => pid,
                                };

                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            dirs_to_process.push((p, child_pid));
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn vfs_get_breadcrumbs(state: State<'_, AppState>) -> Result<Vec<Breadcrumb>, String> {
    let vfs_guard = state.vfs.lock().await;
    let stack = state.pid_stack.lock().await;
    let current_pid = *stack.last().unwrap_or(&0);

    if let Some(tree) = &*vfs_guard {
        let mut path = Vec::new();
        let mut current = current_pid;

        // Walk up the VFS tree to build the path
        while current != 0 {
            if let Some(node) = tree.nodes.get(&current) {
                path.push(Breadcrumb {
                    id: current,
                    name: node.name.clone(),
                });
                current = node.pid;
            } else {
                break;
            }
        }

        // Always add Root at the top
        path.push(Breadcrumb {
            id: 0,
            name: "All Files".to_string(),
        });
        path.reverse();

        Ok(path)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_batch_delete(
    ids: Vec<u64>,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let lanzou = state.lanzou.lock().await;
    let mut vfs_guard = state.vfs.lock().await;

    if let Some(tree) = vfs_guard.as_mut() {
        let targets = get_descendants(tree, &ids, false);
        let now = crate::heriheri::current_timestamp();

        for id in targets {
            if let Some(node) = tree.nodes.get(&id).cloned() {
                if node.is_trashed {
                    continue;
                }

                let is_dir = node.node_type == crate::heriheri::NodeType::Directory;
                let is_chunked = node.node_type == crate::heriheri::NodeType::File
                    && node.chunks != "1"
                    && !node.chunks.is_empty();
                let is_physical_folder = is_dir || is_chunked;

                if is_physical_folder {
                    let _ = lanzou.delete_folder(node.lanzou_id).await;
                } else {
                    let _ = lanzou.delete_file(node.lanzou_id).await;
                }

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                if let Some(mut_node) = tree.nodes.get_mut(&id) {
                    mut_node.is_trashed = true;
                    mut_node.time = now;
                }
            }
        }
        tree.save_local()?;
        Ok(true)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_move_items(
    item_ids: Vec<u64>,
    target_pid: u64,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
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

        let target_lanzou_id = if depth >= 2 {
            tree.deeperdir_lanzou_id.clone()
        } else if target_pid == 0 {
            tree.root_lanzou_id.clone()
        } else {
            tree.nodes
                .get(&target_pid)
                .map(|n| n.lanzou_id.clone())
                .unwrap_or(tree.root_lanzou_id.clone())
        };

        for id in item_ids {
            let node_type = tree.nodes.get(&id).map(|n| n.node_type.clone());

            if let Some(NodeType::Directory) = node_type {
                // If it's a folder, trigger the Recursive Rebuilder!
                rebuild_folder_recursive(tree, &lanzou, id, target_pid, target_lanzou_id.clone())
                    .await?;
            } else if let Some(node) = tree.nodes.get(&id).cloned() {
                if node.lanzou_id.starts_with("alien://") {
                } else if node.chunks != "1" && !node.chunks.is_empty() {
                    let res = lanzou
                        .create_folder_in_target(
                            node.md5.clone(),
                            "".to_string(),
                            target_lanzou_id.clone(),
                        )
                        .await?;

                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                    let new_chunk_folder_id = res["text"].as_str().unwrap_or("").to_string();

                    if !new_chunk_folder_id.is_empty() {
                        // Move the internal .iso parts
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
                        // Delete old wrapper folder
                        let _ = lanzou.delete_folder(node.lanzou_id.clone()).await;

                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                        if let Some(mut_node) = tree.nodes.get_mut(&id) {
                            mut_node.lanzou_id = new_chunk_folder_id;
                        }
                    }
                } else {
                    // --- Standard Single File Move ---
                    let _ = lanzou
                        .move_item(node.lanzou_id.clone(), target_lanzou_id.clone())
                        .await;

                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }

                // Update the parent ID in our local VFS SQLite/JSON immediately
                if let Some(mut_node) = tree.nodes.get_mut(&id) {
                    mut_node.pid = target_pid;
                    mut_node.time = crate::heriheri::current_timestamp();
                }
            }
        }
        tree.save_local()?;
        Ok(true)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_list_bin(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<crate::heriheri::VfsNode>, String> {
    let vfs_guard = state.vfs.lock().await;
    if let Some(tree) = &*vfs_guard {
        let bin_nodes: Vec<crate::heriheri::VfsNode> = tree
            .nodes
            .values()
            .filter(|n| n.is_trashed && !n.is_deleted)
            .cloned()
            .collect();
        Ok(bin_nodes)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_restore_items(
    ids: Vec<u64>,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let lanzou = state.lanzou.lock().await;
    let formhash = lanzou.get_formhash().await?;
    let mut vfs_guard = state.vfs.lock().await;

    if let Some(tree) = vfs_guard.as_mut() {
        let targets = get_descendants(tree, &ids, true);
        let now = crate::heriheri::current_timestamp();

        for id in targets {
            if let Some(node) = tree.nodes.get(&id).cloned() {
                if !node.is_trashed {
                    continue;
                }

                let is_dir = node.node_type == crate::heriheri::NodeType::Directory;
                let is_chunked = node.node_type == crate::heriheri::NodeType::File
                    && node.chunks != "1"
                    && !node.chunks.is_empty();
                let is_physical_folder = is_dir || is_chunked;

                if lanzou
                    .restore_item(&node.lanzou_id, is_physical_folder, &formhash)
                    .await
                    .unwrap_or(false)
                {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                    if !is_physical_folder {
                        let target_lanzou_id = if node.pid == 0 {
                            tree.root_lanzou_id.clone()
                        } else {
                            tree.nodes
                                .get(&node.pid)
                                .map(|n| n.lanzou_id.clone())
                                .unwrap_or(tree.root_lanzou_id.clone())
                        };

                        let _ = lanzou
                            .move_item(node.lanzou_id.clone(), target_lanzou_id)
                            .await;

                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }

                    if let Some(mut_node) = tree.nodes.get_mut(&id) {
                        mut_node.is_trashed = false;
                        mut_node.time = now;
                    }
                }
            }
        }
        tree.save_local()?;
        Ok(true)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_hard_delete_items(
    ids: Vec<u64>,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let lanzou = state.lanzou.lock().await;
    let formhash = lanzou.get_formhash().await?;
    let mut vfs_guard = state.vfs.lock().await;

    if let Some(tree) = vfs_guard.as_mut() {
        let targets = get_descendants(tree, &ids, false);

        for id in targets {
            if let Some(node) = tree.nodes.get(&id).cloned() {
                if !node.is_trashed {
                    continue;
                }
                let is_dir = node.node_type == crate::heriheri::NodeType::Directory;
                let is_chunked = node.node_type == crate::heriheri::NodeType::File
                    && node.chunks != "1"
                    && !node.chunks.is_empty();
                let is_physical_folder = is_dir || is_chunked;

                if !node.lanzou_id.starts_with("alien://") {
                    let _ = lanzou
                        .hard_delete_item(&node.lanzou_id, is_physical_folder, &formhash)
                        .await;

                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }

        // Once physically deleted on the cloud, wipe the requested root IDs locally
        // (tree.delete_node handles recursive local wiping automatically)
        for id in ids {
            tree.delete_node(id);
        }
        tree.save_local()?;
        Ok(true)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_rename_item(
    id: u64,
    new_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let mut vfs_guard = state.vfs.lock().await;

    if let Some(tree) = vfs_guard.as_mut() {
        if let Some(node) = tree.nodes.get_mut(&id) {
            node.name = new_name;
            node.time = crate::heriheri::current_timestamp();
        } else {
            return Err("Item not found in local VFS".to_string());
        }

        tree.save_local()?;
        Ok(true)
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

#[tauri::command]
pub async fn vfs_get_folder_tree(
    id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FlatFile>, String> {
    let vfs_guard = state.vfs.lock().await;
    if let Some(tree) = &*vfs_guard {
        let mut out = Vec::new();
        if let Some(node) = tree.nodes.get(&id) {
            flatten_tree(tree, id, node.name.clone(), &mut out);
        }
        Ok(out)
    } else {
        Err("VFS Offline".to_string())
    }
}

#[tauri::command]
pub async fn vfs_download_file(
    task_id: String,
    vfs_id: u64,
    share_code: Option<String>,
    local_path: String,
    resume_offset: usize,
    total_size: usize,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let task_flag = Arc::new(AtomicU8::new(0));
    state
        .task_ctrl
        .lock()
        .await
        .insert(task_id.clone(), task_flag.clone());

    let (chunks_str, share_url, file_pwd) = {
        if let Some(code) = share_code.filter(|c| !c.is_empty()) {
            let encoded = code.replace("heri://", "");
            let json_str = decrypt_payload(&encoded)
                .map_err(|_| "Failed to decrypt share code".to_string())?;
            let payload = serde_json::from_str::<SharePayload>(&json_str)
                .map_err(|_| "Failed to parse JSON".to_string())?;

            (
                payload.c.to_string(),
                payload.l,
                Some(payload.p).filter(|p| !p.is_empty()),
            )
        } else {
            let vfs_guard = state.vfs.lock().await;
            let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;
            let node = tree.nodes.get(&vfs_id).cloned().ok_or("Node not found")?;

            if node.lanzou_id.starts_with("alien://") {
                let encoded = node.lanzou_id.replace("alien://", "");
                let json_str = decrypt_payload(&encoded)
                    .map_err(|_| "Failed to decrypt Alien payload".to_string())?;
                let payload = serde_json::from_str::<SharePayload>(&json_str)
                    .map_err(|_| "Failed to parse JSON".to_string())?;

                (
                    node.chunks,
                    payload.l,
                    Some(payload.p).filter(|p| !p.is_empty()),
                )
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
                (node.chunks, url, pwd)
            }
        }
    };

    let downloader = state.downloader.lock().await.clone();

    // --- MINIMUM FIX: Attach standard browser headers to bypass CDN Range-Request blocks ---
    let req_client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .unwrap();

    let mut current_loaded = resume_offset;

    // --- CASE A: SINGLE FILE DOWNLOAD ---
    if chunks_str == "1" || chunks_str.is_empty() {
        let direct_url = downloader
            .get_lanzou_direct_link(&share_url, file_pwd.as_deref())
            .await?;

        let mut req = req_client.get(&direct_url);
        if resume_offset > 0 {
            req = req.header("Range", format!("bytes={}-", resume_offset));
        }

        let mut resp = req.send().await.map_err(|e| e.to_string())?;

        // --- MINIMUM FIX: Throw explicit error if CDN rejects the range request ---
        if !resp.status().is_success() {
            return Err(format!("CDN Error: HTTP {}", resp.status()));
        }

        if let Some(parent) = std::path::Path::new(&local_path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }

        // --- MINIMUM FIX: Truncate file when starting from 0 to prevent byte overlap ---
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(resume_offset > 0)
            .truncate(resume_offset == 0)
            .open(&local_path)
            .await
            .map_err(|e| e.to_string())?;

        let mut start_time = tokio::time::Instant::now();

        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            let flag = task_flag.load(Ordering::SeqCst);
            if flag == 1 {
                return Err(format!("PAUSED:{}", current_loaded));
            }
            if flag == 2 {
                return Err("CANCELLED".to_string());
            }

            let limit_kb = state.download_limit.load(Ordering::Relaxed);
            if limit_kb > 0 {
                let expected_time = std::time::Duration::from_secs_f64(
                    chunk.len() as f64 / (limit_kb * 1024) as f64,
                );
                let elapsed = start_time.elapsed();
                if elapsed < expected_time {
                    tokio::time::sleep(expected_time - elapsed).await;
                }
            }
            start_time = tokio::time::Instant::now();

            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            current_loaded += chunk.len();

            let _ = app.emit(
                "download_progress",
                ProgressPayload {
                    task_id: task_id.clone(),
                    loaded: current_loaded,
                    total: total_size,
                },
            );
        }
    }
    // --- CASE B: CHUNKED FILE REASSEMBLY ---
    else {
        let mut all_files = downloader
            .get_lanzou_folder_links(&share_url, file_pwd.as_deref(), 5)
            .await?;

        let re_part = regex::Regex::new(r"_part(\d+)\.iso").unwrap();
        all_files.sort_by(|a, b| {
            let na = a.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let nb = b.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let num_a = re_part
                .captures(na)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            let num_b = re_part
                .captures(nb)
                .and_then(|c| c[1].parse::<u32>().ok())
                .unwrap_or(0);
            num_a.cmp(&num_b)
        });

        let chunk_size = 100 * 1024 * 1024; // 100MB EXACT
        let start_chunk_idx = resume_offset / chunk_size;
        let mut part_resume_offset = resume_offset % chunk_size;

        for i in start_chunk_idx..all_files.len() {
            let direct_url = all_files[i]
                .get("direct_url")
                .and_then(|u| u.as_str())
                .unwrap_or("");

            if direct_url.is_empty() || direct_url == "null" {
                return Err(format!(
                    "Failed to resolve direct URL for chunk: {}",
                    all_files[i]
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("Unknown")
                ));
            }

            let mut req = req_client.get(direct_url);
            if part_resume_offset > 0 {
                req = req.header("Range", format!("bytes={}-", part_resume_offset));
            }

            let mut resp = req.send().await.map_err(|e| e.to_string())?;

            if !resp.status().is_success() {
                return Err(format!(
                    "CDN Error on Chunk {}: HTTP {}",
                    i + 1,
                    resp.status()
                ));
            }

            if let Some(parent) = std::path::Path::new(&local_path).parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| e.to_string())?;
            }

            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(current_loaded > 0)
                .truncate(current_loaded == 0)
                .open(&local_path)
                .await
                .map_err(|e| e.to_string())?;

            let mut start_time = tokio::time::Instant::now();

            while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
                let flag = task_flag.load(Ordering::SeqCst);
                if flag == 1 {
                    return Err(format!("PAUSED:{}", current_loaded));
                }
                if flag == 2 {
                    return Err("CANCELLED".to_string());
                }

                let limit_kb = state.download_limit.load(Ordering::Relaxed);
                if limit_kb > 0 {
                    let expected_time = std::time::Duration::from_secs_f64(
                        chunk.len() as f64 / (limit_kb * 1024) as f64,
                    );
                    let elapsed = start_time.elapsed();
                    if elapsed < expected_time {
                        tokio::time::sleep(expected_time - elapsed).await;
                    }
                }
                start_time = tokio::time::Instant::now();

                file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                current_loaded += chunk.len();

                let _ = app.emit(
                    "download_progress",
                    ProgressPayload {
                        task_id: task_id.clone(),
                        loaded: current_loaded,
                        total: total_size,
                    },
                );
            }
            part_resume_offset = 0;
        }
    }

    state.task_ctrl.lock().await.remove(&task_id);
    Ok(())
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

async fn execute_sync_pull(state: &AppState) -> Result<bool, String> {
    // --- MINIMUM FIX: Removed root_lanzou_id from the extraction ---
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

#[tauri::command]
pub async fn vfs_sync_pull(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    // Secure the gatekeeper lock so Pulls don't overlap with Pushes
    let _guard = state.sync_lock.lock().await;
    execute_sync_pull(&state).await
}

#[tauri::command]
pub async fn vfs_sync_push(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<bool, String> {
    let _guard = state.sync_lock.lock().await;
    let _ = execute_sync_pull(&state).await;

    let (lanzou, tsv_content, new_timestamp) = {
        let lanzou_guard = state.lanzou.lock().await;
        let vfs_guard = state.vfs.lock().await;
        let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;

        (lanzou_guard.clone(), tree.to_tsv(), tree.last_modified)
    };

    let sync_folder_id = get_sync_folder_id(&lanzou).await?;
    let tsv_bytes = tsv_content.into_bytes();
    let total_size = tsv_bytes.len();
    let phone = state.current_phone.lock().await.clone();
    let file_prefix = if phone.is_empty() {
        "heriheri_tree_".to_string()
    } else {
        format!("heriheri_tree_{}_", phone)
    };
    let file_name = format!("{}{}.txt", file_prefix, new_timestamp);

    println!("[SYNC] Pushing new state to cloud: {}", file_name);

    let dummy_flag = Arc::new(AtomicU8::new(0));
    let upload_res = lanzou
        .upload_file_direct(
            tsv_bytes,
            file_name.clone(),
            sync_folder_id.clone(),
            app,
            "SYNC_TASK".to_string(),
            0,
            total_size,
            dummy_flag,
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        )
        .await;

    let new_file_id = match upload_res {
        Ok(id) => id,
        Err(e) => return Err(format!("Sync Push Failed: {}", e)),
    };

    let files_after = lanzou.list_files_by_id(&sync_folder_id).await?;
    for f in files_after {
        if let Some(name) = f["name"].as_str() {
            if name.starts_with(&file_prefix) {
                if let Some(old_id) = f["id"].as_str() {
                    if old_id != new_file_id {
                        let _ = lanzou.delete_file(old_id.to_string()).await;
                    }
                }
            }
        }
    }

    Ok(true)
}

#[tauri::command]
pub async fn vfs_update_speed_limits(
    upload_limit: u32,
    download_limit: u32,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state
        .upload_limit
        .store(upload_limit, std::sync::atomic::Ordering::SeqCst);
    state
        .download_limit
        .store(download_limit, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn vfs_generate_share_code(
    vfs_id: u64,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (node, lanzou) = {
        let vfs_guard = state.vfs.lock().await;
        let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;
        let node = tree.nodes.get(&vfs_id).cloned().ok_or("Node not found")?;
        let lanzou = state.lanzou.lock().await.clone();
        (node, lanzou)
    };

    if node.node_type == crate::heriheri::NodeType::Directory {
        return Err("Renting whole folders is not currently supported.".into());
    }

    // --- Support endless sharing of Rented files ---
    if node.lanzou_id.starts_with("alien://") {
        let encoded = node.lanzou_id.replace("alien://", "");
        return Ok(format!("heri://{}", encoded)); // It's already packed!
    }

    // --- Generate from native files ---
    let is_chunked = node.chunks != "1" && !node.chunks.is_empty();
    let share_info = lanzou
        .get_share_info(node.lanzou_id.clone(), is_chunked)
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

    if url.is_empty() || url == "/" {
        return Err("Could not generate Lanzou link".into());
    }

    let pwd = share_info["pwd"].as_str().unwrap_or("").to_string();
    let chunks_u32 = node.chunks.parse::<u32>().unwrap_or(1);

    let payload = SharePayload {
        n: node.name,
        m: node.md5,
        s: node.size,
        c: chunks_u32,
        l: url,
        p: pwd,
    };

    let json_str = serde_json::to_string(&payload).unwrap();
    let encoded = encrypt_payload(&json_str);
    Ok(format!("heri://{}", encoded))
}

#[tauri::command]
pub fn vfs_resolve_share_code(code: String) -> Result<ResolveResult, String> {
    if !code.starts_with("heri://") {
        return Err("Invalid share code format.".into());
    }

    let encoded = code.replace("heri://", "");
    let json_str = decrypt_payload(&encoded)?;
    let payload: SharePayload =
        serde_json::from_str(&json_str).map_err(|_| "Failed to parse JSON".to_string())?;

    Ok(ResolveResult {
        name: payload.n,
        size: payload.s,
        md5: payload.m,
        chunks: payload.c,
        is_folder: false, // We currently block folder generation
    })
}

#[tauri::command]
pub async fn vfs_rent_item(
    code: String,
    target_pid: u64,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !code.starts_with("heri://") {
        return Err("Invalid share code format.".into());
    }
    let encoded = code.replace("heri://", "");
    let json_str = decrypt_payload(&encoded)?;
    let payload: SharePayload =
        serde_json::from_str(&json_str).map_err(|_| "Failed to parse JSON".to_string())?;

    let mut vfs_guard = state.vfs.lock().await;
    let tree = vfs_guard.as_mut().ok_or("VFS Offline")?;

    // --- RULE 1: Prevent Duplicates in the same folder ---
    if tree
        .nodes
        .values()
        .any(|n| n.pid == target_pid && n.md5 == payload.m && !n.is_deleted && !n.is_trashed)
    {
        return Ok(()); // Silently succeed without adding a second file
    }

    // --- RULE 2: Instant Copy from Physical (Ignore Alien links) ---
    let physical_copy = tree
        .nodes
        .values()
        .find(|n| {
            n.md5 == payload.m
                && n.node_type == crate::heriheri::NodeType::File
                && !n.is_deleted
                && !n.is_trashed
                && !n.lanzou_id.starts_with("alien://")
        })
        .map(|n| (n.lanzou_id.clone(), n.chunks.clone()));

    let ext = std::path::Path::new(&payload.n)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if let Some((phys_id, phys_chunks)) = physical_copy {
        let chunks = phys_chunks.parse::<u32>().unwrap_or(1);
        tree.add_file(
            target_pid, &payload.n, &phys_id, &payload.s, &payload.m, &ext, chunks,
        );
    } else {
        // Only generate an Alien Symlink if we absolutely have to
        let alien_id = format!("alien://{}", encoded);
        tree.add_file(
            target_pid, &payload.n, &alien_id, &payload.s, &payload.m, &ext, payload.c,
        );
    }

    tree.save_local()?;
    Ok(())
}

#[tauri::command]
pub async fn vfs_search(
    query: String,
    state: tauri::State<'_, crate::lanzou::AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let vfs_guard = state.vfs.lock().await;
    let tree = vfs_guard.as_ref().ok_or("VFS Offline")?;
    let q = query.to_lowercase();
    let mut results = Vec::new();

    for node in tree.nodes.values() {
        if !node.is_deleted && !node.is_trashed && node.name.to_lowercase().contains(&q) {
            // Build the string path recursively upwards
            let mut path_parts = Vec::new();
            let mut current_pid = node.pid;

            while let Some(parent) = tree.nodes.get(&current_pid) {
                path_parts.push(parent.name.clone());
                current_pid = parent.pid;
            }
            path_parts.push("All Files".to_string());
            path_parts.reverse();

            // Merge the path_str into the standard node JSON response
            let mut json_node = serde_json::to_value(node).map_err(|e| e.to_string())?;
            json_node["path_str"] = serde_json::Value::String(path_parts.join(" > "));
            results.push(json_node);
        }
    }

    Ok(results)
}
