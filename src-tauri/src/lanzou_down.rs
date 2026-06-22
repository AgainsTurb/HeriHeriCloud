use futures::stream::{self, StreamExt};
use regex::Regex;
use reqwest::cookie::Jar;
use reqwest::Url;
use reqwest::{header, Client};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

fn hexdec(hex_str: &str) -> u32 {
    u32::from_str_radix(hex_str, 16).unwrap_or(0)
}

fn acw_sc_v2_simple(arg1: &str) -> String {
    let pos_list = [
        15, 35, 29, 24, 33, 16, 1, 38, 10, 9, 19, 31, 40, 27, 22, 23, 25, 13, 6, 11, 39, 18, 20, 8,
        14, 21, 32, 26, 2, 30, 7, 4, 17, 5, 3, 28, 34, 37, 12, 36,
    ];
    let mask = "3000176000856006061501533003690027800375";
    let mut out = vec![' '; 40];
    let chars: Vec<char> = arg1.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        for (j, &pos) in pos_list.iter().enumerate() {
            if pos == i + 1 && i < chars.len() {
                out[j] = ch;
            }
        }
    }

    let arg2: String = out.into_iter().collect();
    let mut result = String::new();
    let length = std::cmp::min(arg2.len(), mask.len());

    for i in (0..length).step_by(2) {
        let str_hex = &arg2[i..i + 2];
        let mask_hex = &mask[i..i + 2];
        let xor_val = hexdec(str_hex) ^ hexdec(mask_hex);
        result.push_str(&format!("{:02x}", xor_val));
    }
    result
}

fn get_ajax_headers(referer_url: &str) -> header::HeaderMap {
    let mut headers = header::HeaderMap::new();
    let parsed = Url::parse(referer_url).unwrap();
    let origin = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));

    headers.insert(
        "Accept",
        "application/json, text/javascript, */*".parse().unwrap(),
    );
    headers.insert(
        "Accept-Language",
        "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".parse().unwrap(),
    );
    headers.insert("Cache-Control", "no-cache".parse().unwrap());
    headers.insert(
        "Content-Type",
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    headers.insert("Origin", origin.parse().unwrap());
    headers.insert("Pragma", "no-cache".parse().unwrap());
    headers.insert("Referer", referer_url.parse().unwrap());
    headers.insert("Sec-Fetch-Dest", "empty".parse().unwrap());
    headers.insert("Sec-Fetch-Mode", "cors".parse().unwrap());
    headers.insert("Sec-Fetch-Site", "same-origin".parse().unwrap());
    headers.insert("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36".parse().unwrap());
    headers.insert("X-Requested-With", "XMLHttpRequest".parse().unwrap());

    headers
}

#[derive(Clone)]
pub struct LanzouDownloader {
    pub client: Client,
    pub jar: Arc<Jar>,
}

impl LanzouDownloader {
    pub fn new() -> Self {
        let jar = Arc::new(Jar::default());
        let mut headers = header::HeaderMap::new();
        headers.insert("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36".parse().unwrap());
        headers.insert(
            "Accept-Language",
            "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".parse().unwrap(),
        );

        let client = Client::builder()
            .cookie_provider(Arc::clone(&jar))
            .default_headers(headers)
            .build()
            .unwrap();

        Self { client, jar }
    }

    async fn solve_lanzou_challenge(&self, html: &str, url: &str) -> Result<String, String> {
        let re = Regex::new(r"var arg1='([A-F0-9]+)'").unwrap();
        if let Some(caps) = re.captures(html) {
            let arg1 = &caps[1];
            let cookie_value = acw_sc_v2_simple(arg1);
            let parsed_url = Url::parse(url).unwrap();
            let domain = parsed_url.domain().unwrap_or("");

            self.jar.add_cookie_str(
                &format!("acw_sc__v2={}; Domain={}; Path=/", cookie_value, domain),
                &parsed_url,
            );

            let resp = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let html2 = resp.text().await.map_err(|e| e.to_string())?;
            return Ok(html2);
        }
        Ok(html.to_string())
    }

    async fn resolve_file_page(
        &self,
        share_url: &str,
        password: Option<&str>,
    ) -> Result<(String, String), String> {
        let resp = self
            .client
            .get(share_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let html_initial = resp.text().await.map_err(|e| e.to_string())?;
        let html = self
            .solve_lanzou_challenge(&html_initial, share_url)
            .await?;

        // Type A
        if html.contains("/ajaxm.php?file=")
            && html.replace(" ", "").contains("action':'downprocess'")
            && !html.contains("websignkey")
        {
            let re_comments = Regex::new(r"(?s)/\*.*?\*/").unwrap();
            let html_clean = re_comments.replace_all(&html, "");

            let file_id_re = Regex::new(r"url\s*:\s*'/ajaxm\.php\?file=(\d+)'").unwrap();
            let sign_re =
                Regex::new(r"'action'\s*:\s*'downprocess'\s*,\s*'sign'\s*:\s*'([^']+)'").unwrap();

            let file_id = file_id_re
                .captures(&html_clean)
                .ok_or("Type A: missing file ID")?[1]
                .to_string();
            let sign = sign_re
                .captures(&html_clean)
                .ok_or("Type A: missing sign")?[1]
                .to_string();

            let ajax_url = Url::parse(share_url)
                .unwrap()
                .join(&format!("/ajaxm.php?file={}", file_id))
                .unwrap()
                .to_string();

            let mut post_data = HashMap::new();
            post_data.insert("action", "downprocess");
            post_data.insert("sign", &sign);
            post_data.insert("kd", "1");
            post_data.insert("p", password.unwrap_or(""));

            let resp = self
                .client
                .post(&ajax_url)
                .headers(get_ajax_headers(share_url))
                .form(&post_data)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let result: Value = resp.json().await.map_err(|e| e.to_string())?;

            if result.get("zt").and_then(|z| z.as_i64()) != Some(1) {
                return Err(format!("Type A first step failed: {}", result));
            }

            let dom = result
                .get("dom")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let url_path = result
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            return Ok((dom, url_path));
        }
        // Type B
        else if html.contains("/fn?") && !html.contains("wp_sign") {
            let fn_re = Regex::new(r#"src=["'](/fn\?[^"']+)"#).unwrap();
            let fn_match = fn_re
                .captures(&html)
                .ok_or("Type B: cannot find /fn? URL")?[1]
                .to_string();
            let fn_url = Url::parse(share_url)
                .unwrap()
                .join(&fn_match)
                .unwrap()
                .to_string();

            let mut req_headers = header::HeaderMap::new();
            req_headers.insert("Referer", share_url.parse().unwrap());

            let resp = self
                .client
                .get(&fn_url)
                .headers(req_headers)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let html_fn = resp.text().await.map_err(|e| e.to_string())?;

            let ajaxdata_re = Regex::new(r"var\s+ajaxdata\s*=\s*'([^']+)'").unwrap();
            let wp_sign_re = Regex::new(r"var\s+wp_sign\s*=\s*'([^']+)'").unwrap();
            let file_re = Regex::new(r"url\s*:\s*'/ajaxm\.php\?file=(\d+)'").unwrap();

            let ajaxdata = ajaxdata_re
                .captures(&html_fn)
                .ok_or("Type B: missing ajaxdata")?[1]
                .to_string();
            let wp_sign = wp_sign_re
                .captures(&html_fn)
                .ok_or("Type B: missing wp_sign")?[1]
                .to_string();
            let file_id = file_re
                .captures(&html_fn)
                .ok_or("Type B: missing file_id")?[1]
                .to_string();

            let killdns_re = Regex::new(r"(var\s+killdns|killdns\s*=)").unwrap();
            let kd = if killdns_re.is_match(&html_fn) {
                "1"
            } else {
                "0"
            };

            let ajax_url = Url::parse(share_url)
                .unwrap()
                .join(&format!("/ajaxm.php?file={}", file_id))
                .unwrap()
                .to_string();

            let mut post_data = HashMap::new();
            post_data.insert("action", "downprocess");
            post_data.insert("websignkey", &ajaxdata);
            post_data.insert("signs", &ajaxdata);
            post_data.insert("sign", &wp_sign);
            post_data.insert("websign", "2");
            post_data.insert("kd", kd);
            post_data.insert("ves", "1");

            let resp = self
                .client
                .post(&ajax_url)
                .headers(get_ajax_headers(&fn_url))
                .form(&post_data)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let result: Value = resp.json().await.map_err(|e| e.to_string())?;

            if result.get("zt").and_then(|z| z.as_i64()) != Some(1) {
                return Err(format!("Type B first step failed: {}", result));
            }

            let dom = result
                .get("dom")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let url_path = result
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            return Ok((dom, url_path));
        }

        Err("Unknown page type – not a valid Lanzou file page".to_string())
    }

    async fn get_direct_link_from_dom_url(
        &self,
        dom: &str,
        url_path: &str,
        referer: &str,
    ) -> Result<String, String> {
        let download_page_url = format!("{}/file/{}", dom.trim_end_matches('/'), url_path);
        let parsed_dl_url = Url::parse(&download_page_url).unwrap();
        let domain = parsed_dl_url.domain().unwrap_or("");

        self.jar.add_cookie_str(
            &format!("down_ip=1; Domain={}; Path=/", domain),
            &parsed_dl_url,
        );

        let mut get_headers = header::HeaderMap::new();
        get_headers.insert("Referer", referer.parse().unwrap());

        let resp = self
            .client
            .get(&download_page_url)
            .headers(get_headers)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let html2 = resp.text().await.map_err(|e| e.to_string())?;

        let re_comments = Regex::new(r"(?s)/\*.*?\*/").unwrap();
        let html2_clean = re_comments.replace_all(&html2, "");
        let re_line_comments = Regex::new(r"//.*?\n").unwrap();
        let html2_clean = re_line_comments.replace_all(&html2_clean, "\n");

        let regex_second = Regex::new(
            r"'file'\s*:\s*'([^']+)'\s*,\s*'el'\s*:\s*[a-zA-Z0-9_]+\s*,\s*'sign'\s*:\s*'([^']+)'",
        )
        .unwrap();

        let mut file_val = String::new();
        let mut sign2 = String::new();
        let mut found = false;

        // Emulate Python's [-1] behavior by keeping the last match
        for caps in regex_second.captures_iter(&html2_clean) {
            file_val = caps[1].to_string();
            sign2 = caps[2].to_string();
            found = true;
        }

        if !found {
            return Err("Could not find verification parameters on download page.".to_string());
        }

        let final_ajax_url = parsed_dl_url.join("ajax.php").unwrap().to_string();
        let mut final_post_data = HashMap::new();
        final_post_data.insert("file", file_val);
        final_post_data.insert("el", "2".to_string());
        final_post_data.insert("sign", sign2);

        // Wait 2.1 seconds exactly like Python
        sleep(Duration::from_millis(2100)).await;

        let resp = self
            .client
            .post(&final_ajax_url)
            .headers(get_ajax_headers(&download_page_url))
            .form(&final_post_data)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let final_result: Value = resp.json().await.map_err(|e| e.to_string())?;
        if final_result.get("zt").and_then(|z| z.as_i64()) != Some(1) {
            return Err(format!("Final verification failed: {}", final_result));
        }

        Ok(final_result
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string())
    }

    pub async fn get_lanzou_direct_link(
        &self,
        share_url: &str,
        password: Option<&str>,
    ) -> Result<String, String> {
        let (dom, url_path) = self.resolve_file_page(share_url, password).await?;
        self.get_direct_link_from_dom_url(&dom, &url_path, share_url)
            .await
    }

    pub async fn get_lanzou_folder_links(
        &self,
        folder_url: &str,
        password: Option<&str>,
        concurrency: usize,
    ) -> Result<Vec<Value>, String> {
        let resp = self
            .client
            .get(folder_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let html_initial = resp.text().await.map_err(|e| e.to_string())?;
        let html = self
            .solve_lanzou_challenge(&html_initial, folder_url)
            .await?;

        let fid_re = Regex::new(r"'fid'\s*:\s*(\d+)").unwrap();
        let uid_re = Regex::new(r"'uid'\s*:\s*'(\d+)'").unwrap();
        let puid_re = Regex::new(r"'puid'\s*:\s*'([^']+)'").unwrap();

        let fid = fid_re.captures(&html).ok_or("Missing fid")?[1].to_string();
        let uid = uid_re.captures(&html).ok_or("Missing uid")?[1].to_string();
        let puid = puid_re.captures(&html).ok_or("Missing puid")?[1].to_string();

        let script_block_re = Regex::new(r"(?s)function file\(\)(.*?)function more\(\)").unwrap();
        let script = script_block_re
            .captures(&html)
            .ok_or("Cannot locate file() function")?[1]
            .to_string();

        let t_var_re = Regex::new(r"'t'\s*:\s*(\w+)\s*[,\}]").unwrap();
        let k_var_re = Regex::new(r"'k'\s*:\s*(\w+)\s*[,\}]").unwrap();

        let t_var = t_var_re
            .captures(&script)
            .ok_or("Cannot find t variable name")?[1]
            .to_string();
        let k_var = k_var_re
            .captures(&script)
            .ok_or("Cannot find k variable name")?[1]
            .to_string();

        let t_val_re =
            Regex::new(&format!(r"var\s+{}\s*=\s*'([^']+)'", regex::escape(&t_var))).unwrap();
        let k_val_re =
            Regex::new(&format!(r"var\s+{}\s*=\s*'([^']+)'", regex::escape(&k_var))).unwrap();

        let t = t_val_re.captures(&html).ok_or("Cannot find t value")?[1].to_string();
        let k = k_val_re.captures(&html).ok_or("Cannot find k value")?[1].to_string();

        let mut all_files = Vec::new();
        let mut pg = 1;
        let parsed_url = Url::parse(folder_url).unwrap();
        let base_file_url = format!(
            "{}://{}",
            parsed_url.scheme(),
            parsed_url.host_str().unwrap()
        );

        loop {
            let mut post_data = HashMap::new();
            post_data.insert("lx", "2".to_string());
            post_data.insert("fid", fid.clone());
            post_data.insert("uid", uid.clone());
            post_data.insert("puid", puid.clone());
            post_data.insert("pg", pg.to_string());
            post_data.insert("rep", "0".to_string());
            post_data.insert("t", t.clone());
            post_data.insert("k", k.clone());
            post_data.insert("up", "1".to_string());
            post_data.insert("ls", "1".to_string());
            post_data.insert("pwd", password.unwrap_or("").to_string());

            let filemore_url = parsed_url
                .join(&format!("/filemoreajax.php?file={}", fid))
                .unwrap()
                .to_string();

            let resp = self
                .client
                .post(&filemore_url)
                .headers(get_ajax_headers(folder_url))
                .form(&post_data)
                .send()
                .await
                .map_err(|e| e.to_string())?;

            let data: Value = resp.json().await.map_err(|e| e.to_string())?;
            if data.get("zt").and_then(|z| z.as_i64()) != Some(1) {
                break;
            }

            if let Some(files) = data.get("text").and_then(|t| t.as_array()) {
                all_files.extend(files.clone());
                if files.len() < 50 {
                    break;
                }
            } else {
                break;
            }

            sleep(Duration::from_millis(500)).await;
            pg += 1;
        }

        // Concurrently resolve URLs
        let downloader = self.clone();

        let mut stream = stream::iter(all_files)
            .map(|file_info| {
                let base_url = base_file_url.clone();
                let dl = downloader.clone();

                async move {
                    let id = file_info
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = file_info
                        .get("name_all")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let size = file_info
                        .get("size")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let time = file_info
                        .get("time")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();

                    let file_share_url = format!("{}/{}", base_url, id);

                    match dl.get_lanzou_direct_link(&file_share_url, None).await {
                        Ok(direct) => {
                            json!({
                                "name": name,
                                "size": size,
                                "time": time,
                                "direct_url": direct
                            })
                        }
                        Err(e) => {
                            json!({
                                "name": name,
                                "size": size,
                                "time": time,
                                "direct_url": Value::Null,
                                "error": e
                            })
                        }
                    }
                }
            })
            .buffer_unordered(concurrency);

        let mut results = Vec::new();
        while let Some(res) = stream.next().await {
            results.push(res);
        }

        Ok(results)
    }
}
