use crate::lanzou::AppState;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

// ========================================================
// MCP PROTOCOL TYPES
// ========================================================
#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Serialize, Debug)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Serialize, Debug)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

// ========================================================
// STDIO SERVER
// ========================================================
pub async fn run_stdio_mcp_server(app: AppHandle) {
    eprintln!("[MCP] Started Sandboxed Agent Protocol Server via STDIO.");

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() { continue; }

        match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => {
                let response = handle_mcp_request(req, app.clone()).await;
                let mut out_str = serde_json::to_string(&response).unwrap();
                out_str.push('\n');
                let _ = stdout.write_all(out_str.as_bytes()).await;
                let _ = stdout.flush().await;
            }
            Err(e) => eprintln!("[MCP] Failed to parse JSON-RPC: {}", e),
        }
    }
}

// ========================================================
// SANDBOX WARDEN (Jail Enforcer)
// ========================================================
async fn get_sandbox_id(app: &AppHandle) -> Result<u64, JsonRpcError> {
    let state = app.state::<AppState>();
    let sandbox_name = "AgentWorkspace";
    
    {
        let vfs_guard = state.vfs.lock().await;
        if let Some(tree) = &*vfs_guard {
            if let Some(n) = tree.nodes.values().find(|n| n.pid == 0 && n.name == sandbox_name && !n.is_deleted && !n.is_trashed) {
                return Ok(n.id);
            }
        } else {
            return Err(JsonRpcError { code: -32000, message: "VFS Offline".into() });
        }
    }
    
    // Auto-Create Sandbox if it doesn't exist
    eprintln!("[MCP] Initializing Agent Sandbox Workspace...");
    match crate::lanzou::internal_create_folder(sandbox_name, 0, &state).await {
        Ok(id) => Ok(id),
        Err(e) => Err(JsonRpcError { code: -32000, message: format!("Failed to build Sandbox: {}", e) })
    }
}

fn is_in_sandbox(tree: &crate::heriheri::VfsTree, target_id: u64, sandbox_id: u64) -> bool {
    if target_id == sandbox_id { return true; }
    let mut curr = target_id;
    for _ in 0..50 { // Prevent infinite loops
        if let Some(node) = tree.nodes.get(&curr) {
            if node.pid == sandbox_id { return true; }
            if node.pid == 0 { return false; } // Hit human root before agent root -> VIOLATION
            curr = node.pid;
        } else {
            return false;
        }
    }
    false
}

// ========================================================
// CORE ROUTER
// ========================================================
pub async fn handle_mcp_request(req: JsonRpcRequest, app: AppHandle) -> JsonRpcResponse {
    let result = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-11-25", 
            "capabilities": { "tools": {}, "prompts": {} },
            "serverInfo": { 
                "name": "heriheri-mcp-sandboxed",
                "version": env!("CARGO_PKG_VERSION") 
            }
        })),
        "notifications/initialized" => Ok(json!({})),
        
        "prompts/list" => Ok(json!({
            "prompts": [{ "name": "heriheri_manual", "description": "Operational rules for HeriHeriCloud" }]
        })),
        "prompts/get" => Ok(json!({
            "description": "HeriHeriCloud Sandboxed Agent Manual",
            "messages": [{ "role": "user", "content": { "type": "text", "text": "1. You are jailed to the 'AgentWorkspace'. To you, PID 0 maps to this workspace.\n2. Always use read_vfs to find the ID of subfolders or files before operating on them.\n3. You can create, rename, and delete items securely." } }]
        })),

        "tools/list" => Ok(json!({
            "tools": [
                {
                    "name": "read_vfs",
                    "description": "List files/folders. Omit folder_pid or pass 0 to read your Workspace root.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "folder_pid": { "type": "number" } }
                    }
                },
                {
                    "name": "search_cloud",
                    "description": "Search for files within your Workspace.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "query": { "type": "string" } },
                        "required": ["query"]
                    }
                },
                {
                    "name": "create_folder",
                    "description": "Create a new folder.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { 
                            "name": { "type": "string" },
                            "target_pid": { "type": "number", "description": "Parent ID (0 for root)" }
                        },
                        "required": ["name", "target_pid"]
                    }
                },
                {
                    "name": "rename_item",
                    "description": "Rename a file or folder by its ID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { 
                            "id": { "type": "number" },
                            "new_name": { "type": "string" }
                        },
                        "required": ["id", "new_name"]
                    }
                },
                {
                    "name": "delete_item",
                    "description": "Move a file or folder to the trash by its ID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "id": { "type": "number" } },
                        "required": ["id"]
                    }
                },
                {
                    "name": "agent_upload",
                    "description": "Upload a local file to the cloud.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { 
                            "local_path": { "type": "string" },
                            "target_pid": { "type": "number", "description": "0 for Workspace root" }
                        },
                        "required": ["local_path", "target_pid"]
                    }
                },
                {
                    "name": "agent_download",
                    "description": "Download a file from the cloud to local disk.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { 
                            "vfs_id": { "type": "number" },
                            "local_path": { "type": "string" }
                        },
                        "required": ["vfs_id", "local_path"]
                    }
                }
            ]
        })),

        "tools/call" => {
            let params = req.params.unwrap_or(json!({}));
            let name = params["name"].as_str().unwrap_or("");
            let args = params["arguments"].clone();

            match name {
                "read_vfs" => tool_read_vfs(args, app).await,
                "search_cloud" => tool_search_cloud(args, app).await,
                "create_folder" => tool_create_folder(args, app).await,
                "rename_item" => tool_rename_item(args, app).await,
                "delete_item" => tool_delete_item(args, app).await,
                "agent_upload" => tool_agent_upload(args, app).await,
                "agent_download" => tool_agent_download(args, app).await,
                _ => Err(JsonRpcError { code: -32601, message: format!("Tool {} not found", name) }),
            }
        },
        _ => Err(JsonRpcError { code: -32601, message: format!("Method {} not found", req.method) }),
    };

    match result {
        Ok(res) => JsonRpcResponse { jsonrpc: "2.0".into(), id: req.id, result: Some(res), error: None },
        Err(err) => JsonRpcResponse { jsonrpc: "2.0".into(), id: req.id, result: None, error: Some(err) },
    }
}

// ========================================================
// TOOL IMPLEMENTATIONS 
// ========================================================

async fn tool_read_vfs(args: Value, app: AppHandle) -> Result<Value, JsonRpcError> {
    let sandbox_id = get_sandbox_id(&app).await?;
    let req_pid = args["folder_pid"].as_u64().unwrap_or(0);
    let target_pid = if req_pid == 0 { sandbox_id } else { req_pid };

    let state = app.state::<AppState>();
    let vfs_guard = state.vfs.lock().await;
    let tree = vfs_guard.as_ref().unwrap();

    if !is_in_sandbox(tree, target_pid, sandbox_id) {
        return Err(JsonRpcError { code: -32000, message: "Sandbox Violation: Access Denied".into() });
    }

    let nodes: Vec<_> = tree.nodes.values()
        .filter(|n| n.pid == target_pid && !n.is_trashed && !n.is_deleted)
        .map(|n| {
            let decoded_name = STANDARD.decode(&n.name).ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_else(|| n.name.clone());
            json!({
                "id": n.id,
                "name": decoded_name,
                "type": if n.node_type == crate::heriheri::NodeType::Directory { "folder" } else { "file" },
                "size": n.size,
                "md5": n.md5
            })
        }).collect();
        
    Ok(mcp_text_result(&serde_json::to_string_pretty(&nodes).unwrap()))
}

async fn tool_search_cloud(args: Value, app: AppHandle) -> Result<Value, JsonRpcError> {
    let sandbox_id = get_sandbox_id(&app).await?;
    let query = args["query"].as_str().unwrap_or("").to_string();
    let state = app.state::<AppState>();
    
    let mut results = crate::lanzou::vfs_search(query, state.clone()).await
        .map_err(|e| JsonRpcError { code: -32000, message: e })?;

    // Filter out human files that leak into the search
    let vfs_guard = state.vfs.lock().await;
    let tree = vfs_guard.as_ref().unwrap();
    results.retain(|r| {
        let id = r["id"].as_u64().unwrap_or(0);
        is_in_sandbox(tree, id, sandbox_id)
    });

    Ok(mcp_text_result(&serde_json::to_string_pretty(&results).unwrap()))
}

async fn tool_create_folder(args: Value, app: AppHandle) -> Result<Value, JsonRpcError> {
    let sandbox_id = get_sandbox_id(&app).await?;
    let req_pid = args["target_pid"].as_u64().unwrap_or(0);
    let target_pid = if req_pid == 0 { sandbox_id } else { req_pid };
    let name = args["name"].as_str().unwrap_or("New Folder").to_string();

    let state = app.state::<AppState>();
    {
        let vfs_guard = state.vfs.lock().await;
        let tree = vfs_guard.as_ref().unwrap();
        if !is_in_sandbox(tree, target_pid, sandbox_id) {
            return Err(JsonRpcError { code: -32000, message: "Sandbox Violation: Access Denied".into() });
        }
    }

    match crate::lanzou::internal_create_folder(&name, target_pid, &state).await {
        Ok(new_id) => Ok(mcp_text_result(&format!("Folder created successfully. ID: {}", new_id))),
        Err(e) => Err(JsonRpcError { code: -32000, message: format!("Create folder failed: {}", e) }),
    }
}

async fn tool_rename_item(args: Value, app: AppHandle) -> Result<Value, JsonRpcError> {
    let sandbox_id = get_sandbox_id(&app).await?;
    let id = args["id"].as_u64().unwrap_or(0);
    let new_name = args["new_name"].as_str().unwrap_or("Renamed").to_string();
    let state = app.state::<AppState>();

    {
        let vfs_guard = state.vfs.lock().await;
        let tree = vfs_guard.as_ref().unwrap();
        if !is_in_sandbox(tree, id, sandbox_id) || id == sandbox_id {
            return Err(JsonRpcError { code: -32000, message: "Sandbox Violation: Cannot modify root or external items".into() });
        }
    }

    match crate::lanzou::vfs_rename_item(id, new_name, state).await {
        Ok(_) => Ok(mcp_text_result("Item renamed successfully.")),
        Err(e) => Err(JsonRpcError { code: -32000, message: format!("Rename failed: {}", e) }),
    }
}

async fn tool_delete_item(args: Value, app: AppHandle) -> Result<Value, JsonRpcError> {
    let sandbox_id = get_sandbox_id(&app).await?;
    let id = args["id"].as_u64().unwrap_or(0);
    let state = app.state::<AppState>();

    {
        let vfs_guard = state.vfs.lock().await;
        let tree = vfs_guard.as_ref().unwrap();
        if !is_in_sandbox(tree, id, sandbox_id) || id == sandbox_id {
            return Err(JsonRpcError { code: -32000, message: "Sandbox Violation: Cannot delete root or external items".into() });
        }
    }

    match crate::lanzou::vfs_batch_delete(vec![id], state).await {
        Ok(_) => Ok(mcp_text_result("Item moved to trash.")),
        Err(e) => Err(JsonRpcError { code: -32000, message: format!("Delete failed: {}", e) }),
    }
}

async fn tool_agent_upload(args: Value, app: AppHandle) -> Result<Value, JsonRpcError> {
    let sandbox_id = get_sandbox_id(&app).await?;
    let local_path = args["local_path"].as_str().unwrap_or("").to_string();
    let req_pid = args["target_pid"].as_u64().unwrap_or(0);
    let target_pid = if req_pid == 0 { sandbox_id } else { req_pid };
    
    let state = app.state::<AppState>();
    {
        let vfs_guard = state.vfs.lock().await;
        let tree = vfs_guard.as_ref().unwrap();
        if !is_in_sandbox(tree, target_pid, sandbox_id) {
            return Err(JsonRpcError { code: -32000, message: "Sandbox Violation: Access Denied".into() });
        }
    }

    let task_id = format!("mcp_up_{}", tokio::time::Instant::now().elapsed().as_millis());
    eprintln!("[MCP] Agent initiated upload for {}", local_path);

    match crate::lanzou::vfs_upload_file(local_path, task_id, target_pid, "".into(), 0, app.clone(), state).await {
        Ok(_) => Ok(mcp_text_result("Upload successful. The file is now secure in the cloud.")),
        Err(e) => Err(JsonRpcError { code: -32000, message: format!("Upload failed: {}", e) }),
    }
}

async fn tool_agent_download(args: Value, app: AppHandle) -> Result<Value, JsonRpcError> {
    let sandbox_id = get_sandbox_id(&app).await?;
    let vfs_id = args["vfs_id"].as_u64().unwrap_or(0);
    let local_path = args["local_path"].as_str().unwrap_or("").to_string();
    let state = app.state::<AppState>();

    let total_size = {
        let vfs_guard = state.vfs.lock().await;
        let tree = vfs_guard.as_ref().unwrap();
        if !is_in_sandbox(tree, vfs_id, sandbox_id) {
            return Err(JsonRpcError { code: -32000, message: "Sandbox Violation: Access Denied".into() });
        }
        tree.nodes.get(&vfs_id).map(|n| {
            let s = n.size.to_uppercase().replace(" ", "");
            let val: f64 = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().parse().unwrap_or(0.0);
            let mult = if s.contains("G") { 1024.0*1024.0*1024.0 } else if s.contains("M") { 1024.0*1024.0 } else if s.contains("K") { 1024.0 } else { 1.0 };
            (val * mult) as usize
        }).unwrap_or(0)
    };

    let task_id = format!("mcp_dn_{}", tokio::time::Instant::now().elapsed().as_millis());
    eprintln!("[MCP] Agent initiated download to {}", local_path);

    match crate::lanzou::vfs_download_file(task_id, vfs_id, None, local_path, 0, total_size, app.clone(), state).await {
        Ok(_) => Ok(mcp_text_result("Download complete.")),
        Err(e) => Err(JsonRpcError { code: -32000, message: format!("Download failed: {}", e) }),
    }
}

fn mcp_text_result(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}