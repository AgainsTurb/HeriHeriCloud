import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useTranslation } from "react-i18next";
import { getFileIcon } from "../Utils/fileIcons";

import { DndContext, useDraggable, useDroppable, PointerSensor, useSensor, useSensors, DragEndEvent } from '@dnd-kit/core';

import { useFileSelection } from "../Hooks/useFileSelection";
import { useRectangleSelect } from "../Hooks/useRectangleSelect";
import { useContextMenu } from "../Hooks/useContextMenu";

function FileRowNode({ node, index, isSelected, isCut, formatTime, formatBytes, handleRowClick, navigateToFolder, triggerShare, setShowBatchDelete, setSelectedNodes, handleContextMenu, openMediaWindow }: any) {
  const isDir = node.node_type === "Directory";

  const { attributes, listeners, setNodeRef: setDragRef, transform, isDragging } = useDraggable({
    id: node.id,
    data: { node }
  });

  const { setNodeRef: setDropRef, isOver } = useDroppable({
    id: node.id,
    disabled: !isDir
  });

  const setCombinedRef = (element: HTMLDivElement | null) => {
    setDragRef(element);
    if (isDir) setDropRef(element);
  };

  const style: React.CSSProperties = {
    ...styles.listRow,
    backgroundColor: isOver ? "#d1d5db" : isSelected ? "#e5e7eb" : "transparent",
    opacity: isDragging ? 0.4 : isCut ? 0.5 : 1,
    transform: transform ? `translate3d(${transform.x}px, ${transform.y}px, 0)` : undefined,
    zIndex: isDragging ? 100 : 1,
    position: "relative"
  };

  return (
    <div 
      ref={setCombinedRef}
      style={style}
      {...attributes}
      {...listeners}
      className="file-row"
      onClick={(e) => handleRowClick(e, index, node.id)}
      onDoubleClick={(e) => {
        e.stopPropagation();
        if (isDir) {
          navigateToFolder(node.id);
        } else {
          const ext = node.name.split('.').pop()?.toLowerCase();
          const mediaExts = ['mp4', 'mkv', 'webm', 'ogg', 'mp3', 'wav', 'flac', 'm4a', 'aac'];
          if (mediaExts.includes(ext)) {
            openMediaWindow(node);
          }
        }
      }}
      onContextMenu={(e) => {
        if (!isSelected) setSelectedNodes(new Set([node.id]));
        handleContextMenu(e, node.id);
      }}
    >
      <div style={styles.cellName}>
        <img src={getFileIcon(node.name, isDir)} alt="icon" style={styles.icon} />
        <span style={{...styles.itemName, color: "#111827"}}>{node.name}</span>
      </div>
      <div style={styles.cellDefault}>{isDir ? "-" : formatBytes(node.size)}</div>
      <div style={styles.cellDefault}>{formatTime(node.time)}</div>
      <div style={{...styles.cellDefault, fontSize: "11px", fontFamily: "monospace"}}>{isDir ? "-" : node.md5.substring(0, 16) + "..."}</div>
      <div style={styles.cellActions}>
        <button style={styles.actionBtn} title="Share" onMouseDown={(e) => e.stopPropagation()} onClick={(e) => triggerShare(e, node.id)}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8M16 6l-4-4-4 4M12 2v13"/></svg>
        </button>
        <button style={styles.actionBtn} title="Delete" onMouseDown={(e) => e.stopPropagation()} onClick={(e) => {
          e.stopPropagation();
          if (!isSelected) setSelectedNodes(new Set([node.id]));
          setShowBatchDelete(true);
        }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        </button>
      </div>
    </div>
  );
}

function BreadcrumbNode({ crumb, isLast, navigateToFolder }: any) {
  const { t } = useTranslation();
  const { setNodeRef, isOver } = useDroppable({
    id: crumb.id,
    disabled: isLast
  });

  return (
    <span ref={setNodeRef} style={{ display: "flex", alignItems: "center" }}>
      <span 
        style={{
          ...styles.breadcrumbLink, 
          backgroundColor: isOver ? "#dbeafe" : "transparent",
          padding: "2px 6px",
          borderRadius: "4px",
          transition: "background-color 0.2s"
        }} 
        onClick={(e) => { e.stopPropagation(); navigateToFolder(crumb.id); }}
      >
        {crumb.id === 0 || crumb.name === "All Files" ? t("All Files") : crumb.name}
      </span>
      {!isLast && <span style={{ margin: "0 6px", color: "#9ca3af" }}>/</span>}
    </span>
  );
}

export default function Home({ status }: { status: string }) {
  const { t } = useTranslation();
  const [nodes, setNodes] = useState<any[]>([]);
  const [breadcrumbs, setBreadcrumbs] = useState<{id: number, name: string}[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));

  const [alertData, setAlertData] = useState<{ title: string, msg: string } | null>(null);
  const [showFolderModal, setShowFolderModal] = useState(false);
  const [folderForm, setFolderForm] = useState({ name: "", desc: "" });
  const [shareData, setShareData] = useState<{ url: string, pwd: string } | null>(null);
  const [showBatchDelete, setShowBatchDelete] = useState(false);
  const [clipboard, setClipboard] = useState<{ type: "cut", ids: number[] } | null>(null);

  const triggerBatchDelete = () => setShowBatchDelete(true);
  const triggerCut = (ids: number[]) => setClipboard({ type: "cut", ids });
  const triggerPaste = () => handlePaste();

  const { selectedNodes, setSelectedNodes, handleRowClick, clearSelection } = useFileSelection(nodes, triggerBatchDelete, triggerCut, triggerPaste);
  const selectionBox = useRectangleSelect(containerRef, nodes, setSelectedNodes);
  const { contextMenu, handleContextMenu, closeMenu } = useContextMenu();

  const [showRenameModal, setShowRenameModal] = useState(false);
  const [renameTargetId, setRenameTargetId] = useState<number | null>(null);
  const [renameName, setRenameName] = useState("");

  useEffect(() => {
    if (status === "Connected") initializeDrive();
  }, [status]);

  useEffect(() => {
    const handleTaskEnd = () => fetchDirectory();
    window.addEventListener("TASK_END", handleTaskEnd);
    return () => window.removeEventListener("TASK_END", handleTaskEnd);
  }, []);

  useEffect(() => {
    const unlisten = listen("tauri://drag-drop", async (event: any) => {
      if (status !== "Connected") return;
      const droppedPaths: string[] = event.payload.paths;
      if (!droppedPaths || droppedPaths.length === 0) return;

      showAlert(t("Processing"), t("Scanning folder structure... Please wait."));
      setIsLoading(true);

      const currentPid = await invoke<number>("vfs_get_current_pid").catch(() => 0);
      
      try {
        const safeFiles = await invoke<any[]>("vfs_expand_drop", { paths: droppedPaths, currentPid });
        
        if (safeFiles.length > 0) {
          const activeTasks = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
          const groups = new Map();

          safeFiles.forEach((f: any) => {
            if (f.groupId) {
              if (!groups.has(f.groupId)) {
                groups.set(f.groupId, {
                  id: f.groupId, isGroup: true, name: f.groupName, status: "Queued", totalItems: 0, finishedItems: 0, type: "Upload"
                });
              }
              groups.get(f.groupId).totalItems += 1;
            }
          });
          groups.forEach(g => activeTasks.push(g));

          safeFiles.forEach((fileObj: any) => {
            const fileName = fileObj.path.split(/[/\\]/).pop() || "Unknown File";
            const taskId = "t_" + Date.now().toString() + Math.random().toString(36).substring(2, 7);
            activeTasks.push({ 
              id: taskId, groupId: fileObj.groupId, isGroup: false, name: fileName, type: "Upload", status: "Queued", 
              filePath: fileObj.path, targetPid: fileObj.targetPid, resumeFolder: "", resumeChunk: 0 
            });
          });

          localStorage.setItem("heriheri_active", JSON.stringify(activeTasks));
          window.dispatchEvent(new CustomEvent("TASK_START"));
          
          showAlert(t("Upload Queued"), `${safeFiles.length} ${t("items added to the queue.")}`);
        } else {
          setAlertData(null); // Clear alert if empty drop
        }
      } catch (err) {
        showAlert(t("Drop Error"), t("Failed to process folder: ") + String(err));
      }
      
      fetchDirectory();
      setIsLoading(false);
    });
    return () => { unlisten.then(f => f()); };
  }, [status]);

  const showAlert = (title: string, msg: string) => setAlertData({ title, msg });
  const currentPhone = localStorage.getItem("phone") || "";

  async function initializeDrive() {
    setIsLoading(true);
    try {
      await invoke("init_vfs_root", { phone: currentPhone });
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await fetchDirectory();
    } catch (error) {
      console.error("Failed to init VFS:", error);
      setIsLoading(false);
    }
  }

  async function fetchDirectory() {
    setIsLoading(true);
    clearSelection();
    try {
      const data = await invoke<any[]>("vfs_list_dir");
      setNodes(data);
      const crumbs = await invoke<{id: number, name: string}[]>("vfs_get_breadcrumbs").catch(() => []);
      setBreadcrumbs(crumbs);
    } catch (error) {
      console.error("Directory fetch error:", error);
    } finally {
      setIsLoading(false);
    }
  }

  const openMediaWindow = async (node: any) => {
    const streamUrl = encodeURIComponent(`http://127.0.0.1:8888/stream/${node.id}`);
    const title = encodeURIComponent(node.name);
    const isAudio = ['mp3', 'wav', 'flac', 'm4a', 'aac', 'ogg'].includes(node.name.split('.').pop()?.toLowerCase() || '');

    // Note the /#/ syntax here! This tells HashRouter to load the MediaPlayer.
    const routeUrl = `index.html#/player?url=${streamUrl}&title=${title}&isAudio=${isAudio}`;

    const playerWindow = new WebviewWindow(`player-${node.id}`, {
      url: routeUrl,
      title: `Playing: ${node.name}`,
      width: 854,
      height: isAudio ? 200 : 480,
      center: true,
      resizable: true,
    });

    playerWindow.once('tauri://error', function (e) {
      console.warn("Window might already exist. Focusing instead.");
    });
  };

  const handleDragEnd = async (event: DragEndEvent) => {
    const { active, over } = event;
    if (!over) return;

    const draggedId = active.id as number;
    const targetFolderId = over.id as number;

    if (draggedId === targetFolderId) return;

    const itemIdsToMove = selectedNodes.has(draggedId) ? Array.from(selectedNodes) : [draggedId];
    if (itemIdsToMove.includes(targetFolderId)) return;

    setIsLoading(true);
    try {
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await invoke("vfs_move_items", { itemIds: itemIdsToMove, targetPid: targetFolderId });
      await invoke("vfs_sync_push").catch((e) => console.warn("Sync push skipped:", e));
      
      clearSelection();
      fetchDirectory();
    } catch (err) {
      showAlert("Move Error", String(err));
      setIsLoading(false);
    }
  };

  async function handlePaste() {
    if (!clipboard || clipboard.type !== "cut") return;
    setIsLoading(true);
    try {
      const currentPid = await invoke<number>("vfs_get_current_pid").catch(() => 0);
      
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await invoke("vfs_move_items", { itemIds: clipboard.ids, targetPid: currentPid });
      await invoke("vfs_sync_push").catch((e) => console.warn("Sync push skipped:", e));
      
      setClipboard(null);
      clearSelection();
      fetchDirectory();
    } catch (err) {
      showAlert("Paste Error", String(err));
      setIsLoading(false);
    }
  }

  async function handleRenameSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!renameName || !renameTargetId) return;
    setShowRenameModal(false);
    setIsLoading(true);
    try {
        await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
        await invoke("vfs_rename_item", { id: renameTargetId, newName: renameName });
        await invoke("vfs_sync_push").catch((e) => console.warn("Sync push skipped:", e));
        
        setRenameName("");
        fetchDirectory();
    } catch (error) {
        showAlert("Rename Error", `Failed to rename: ${error}`);
        setIsLoading(false);
    }
  }

  async function confirmBatchDelete() {
    setShowBatchDelete(false);
    setIsLoading(true);
    try {
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await invoke("vfs_batch_delete", { ids: Array.from(selectedNodes) });
      await invoke("vfs_sync_push").catch((e) => console.warn("Sync push skipped:", e));
      
      clearSelection();
      fetchDirectory();
    } catch (error) {
      showAlert("Delete Error", "Failed to delete selected items.");
      setIsLoading(false);
    }
  }

  async function navigateToFolder(id: number) {
    await invoke("vfs_enter_folder", { id });
    fetchDirectory();
  }

  async function handleCreateFolderSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!folderForm.name) return;
    setShowFolderModal(false);
    setIsLoading(true);
    try {
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await invoke("vfs_create_folder", { name: folderForm.name, desc: folderForm.desc });
      await invoke("vfs_sync_push").catch((e) => console.warn("Sync push skipped:", e));
      
      setFolderForm({ name: "", desc: "" });
      fetchDirectory();
    } catch (error) {
      showAlert("Error", `Failed to create folder: ${error}`);
      setIsLoading(false);
    }
  }

  async function handleUpload() {
    try {
      const selected = await open({ multiple: true, title: "Select Files to Upload" });
      if (!selected || selected.length === 0) return;
      
      const paths = Array.isArray(selected) ? selected : [selected];
      const currentPid = await invoke<number>("vfs_get_current_pid").catch(() => 0);
      const activeTasks = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
      
      paths.forEach(filePath => {
        const fileName = filePath.split(/[/\\]/).pop() || "Unknown File";
        const taskId = Date.now().toString() + Math.random().toString(36).substr(2, 5);
        activeTasks.push({ id: taskId, isGroup: false, name: fileName, type: "Upload", status: "Queued", filePath, targetPid: currentPid, resumeFolder: "", resumeChunk: 0 });
      });
      
      localStorage.setItem("heriheri_active", JSON.stringify(activeTasks));
      window.dispatchEvent(new CustomEvent("TASK_START"));
      showAlert("Upload Queued", `${paths.length} file(s) added to the queue.`);
    } catch (error) {
      console.error("Upload dialog error:", error);
    }
  }

  async function triggerShare(e: React.MouseEvent, vfsId: number) {
    e.stopPropagation();
    try {
      const targetIds = selectedNodes.has(vfsId) && selectedNodes.size > 1 ? Array.from(selectedNodes) : [vfsId];
      let codes = [];
      
      for (const id of targetIds) {
        const node = nodes.find(n => n.id === id);
        if (node && node.node_type !== "Directory") {
           const code = await invoke<string>("vfs_generate_share_code", { vfsId: id });
           codes.push(code);
        }
      }
      
      if (codes.length === 0) {
         showAlert(t("Share Error"), t("Folders cannot be shared. Please select files."));
         return;
      }
      
      setShareData({ url: codes.join("\n"), pwd: "" }); 
    } catch (error) {
      showAlert(t("Share Error"), String(error));
    }
  }

  function formatTime(timestampMs: number) {
    if (!timestampMs || timestampMs === 0) return "-";
    const d = new Date(timestampMs);
    return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')} ${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}`;
  }

  function formatBytes(sizeStr: string) {
    if (!sizeStr || sizeStr === "-") return "-";
    
    // If it's a legacy file from Lanzou with letters (e.g., "1.2 M" or "300 K"), return as-is
    if (/[a-zA-Z]/.test(sizeStr)) return sizeStr;

    // Otherwise, parse the exact bytes and format beautifully
    const bytes = parseInt(sizeStr, 10);
    if (isNaN(bytes)) return sizeStr;
    if (bytes === 0) return "0 B";

    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  }

  return (
    <DndContext sensors={sensors} onDragEnd={handleDragEnd}>
      <div 
        style={{ display: "flex", flexDirection: "column", height: "100%", position: "relative" }} 
        onClick={clearSelection} 
        onContextMenu={(e) => handleContextMenu(e, null)}
      >
        <header style={styles.header}>
          <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
            <h2 style={{ margin: 0, textTransform: "uppercase", letterSpacing: "1px", fontSize: "20px" }}>{t("All Files")}</h2>
            <div style={styles.breadcrumbBar}>
              {breadcrumbs.map((crumb, idx) => (
                <BreadcrumbNode 
                  key={crumb.id} 
                  crumb={crumb} 
                  isLast={idx === breadcrumbs.length - 1} 
                  navigateToFolder={navigateToFolder} 
                />
              ))}
            </div>
          </div>
        </header>

        <div style={styles.listContainer} ref={containerRef}>
          <div style={styles.listHeaderRow}>
            <div style={styles.cellName}>{t("Name")}</div>
            <div style={styles.cellDefault}>{t("Size")}</div>
            <div style={styles.cellDefault}>{t("Time")}</div>
            <div style={styles.cellDefault}>{t("MD5 Hash")}</div>
            <div style={styles.cellActions}>{t("Actions")}</div>
          </div>

          {isLoading && <div style={styles.statusState}>{t("SYNCING & LOADING...")}</div>}
          {!isLoading && nodes.length === 0 && status === "Connected" && (
            <div style={styles.statusState}>{t("This folder is empty. Right-click to begin.")}</div>
          )}
          {!isLoading && status !== "Connected" && (
            <div style={styles.statusState}>{t("Please connect to the cloud via the sidebar.")}</div>
          )}

          {!isLoading && status === "Connected" && (
            <div style={styles.listBody}>
              {nodes.map((node, index) => (
                <FileRowNode
                  key={node.id}
                  node={node}
                  index={index}
                  isSelected={selectedNodes.has(node.id)}
                  isCut={clipboard?.ids.includes(node.id)}
                  formatTime={formatTime}
                  formatBytes={formatBytes}
                  handleRowClick={handleRowClick}
                  navigateToFolder={navigateToFolder}
                  triggerShare={triggerShare}
                  setShowBatchDelete={setShowBatchDelete}
                  setSelectedNodes={setSelectedNodes}
                  handleContextMenu={handleContextMenu}
                  openMediaWindow={openMediaWindow}
                />
              ))}
            </div>
          )}

          {selectionBox && (
            <div style={{
              position: "absolute",
              border: "1px solid #111827",
              backgroundColor: "rgba(17, 24, 39, 0.1)",
              pointerEvents: "none",
              left: Math.min(selectionBox.startX, selectionBox.currX),
              top: Math.min(selectionBox.startY, selectionBox.currY),
              width: Math.abs(selectionBox.currX - selectionBox.startX),
              height: Math.abs(selectionBox.currY - selectionBox.startY),
              zIndex: 50
            }} />
          )}
        </div>

        {/* --- ALL POPUPS AND MODALS --- */}
        {alertData && (
          <div style={styles.modalOverlay} onClick={() => setAlertData(null)}>
            <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
              <h3 style={styles.modalTitle}>{alertData.title}</h3>
              <p style={styles.modalText}>{alertData.msg}</p>
              <div style={styles.modalActions}>
                <button style={styles.primaryButton} onClick={() => setAlertData(null)}>{t("OK")}</button>
              </div>
            </div>
          </div>
        )}

        {showFolderModal && (
          <div style={styles.modalOverlay} onClick={() => setShowFolderModal(false)}>
            <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
              <h3 style={styles.modalTitle}>{t("Create New Folder")}</h3>
              <form onSubmit={handleCreateFolderSubmit} style={{ display: "flex", flexDirection: "column", gap: "16px", marginTop: "8px" }}>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Folder Name")}</label>
                  <input style={styles.input} required autoFocus
                    value={folderForm.name} onChange={(e) => setFolderForm({ ...folderForm, name: e.target.value })} />
                </div>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Description")}</label>
                  <input style={styles.input} 
                    value={folderForm.desc} onChange={(e) => setFolderForm({ ...folderForm, desc: e.target.value })} />
                </div>
                <div style={styles.modalActions}>
                  <button type="button" style={styles.secondaryButton} onClick={() => setShowFolderModal(false)}>{t("Cancel")}</button>
                  <button type="submit" style={styles.primaryButton}>{t("Create")}</button>
                </div>
              </form>
            </div>
          </div>
        )}

        {shareData && (
          <div style={styles.modalOverlay} onClick={() => setShareData(null)}>
            <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
              <h3 style={styles.modalTitle}>{t("Share Item")}</h3>
              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("HeriHeri Share Code")}</label>
                <textarea 
                  style={{...styles.readOnlyInput, fontFamily: "monospace", minHeight: "80px", resize: "vertical", whiteSpace: "pre-wrap"}} 
                  value={shareData.url} 
                  readOnly 
                  onClick={e => e.currentTarget.select()} 
                />
              </div>
              <div style={styles.modalActions}>
                <button style={styles.secondaryButton} onClick={() => setShareData(null)}>{t("Close")}</button>
                <button style={styles.primaryButton} onClick={() => {
                  navigator.clipboard.writeText(shareData.url);
                  setShareData(null);
                  showAlert("Copied", "Code copied to clipboard");
                }}>{t("Copy")}</button>
              </div>
            </div>
          </div>
        )}

        {showBatchDelete && (
          <div style={styles.modalOverlay} onClick={() => setShowBatchDelete(false)}>
            <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
              <h3 style={styles.modalTitle}>{t("Confirm Deletion")}</h3>
              <p style={styles.modalText}>
                {t("Are you sure you want to permanently delete")} <strong>{selectedNodes.size} item(s)</strong>{t("? This action cannot be undone.")}
              </p>
              <div style={styles.modalActions}>
                <button style={styles.secondaryButton} onClick={() => setShowBatchDelete(false)}>{t("Cancel")}</button>
                <button style={styles.dangerButton} onClick={confirmBatchDelete}>{t("Delete")}</button>
              </div>
            </div>
          </div>
        )}

        {showRenameModal && (
          <div style={styles.modalOverlay} onClick={() => setShowRenameModal(false)}>
            <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
              <h3 style={styles.modalTitle}>{t("Rename Item")}</h3>
              <form onSubmit={handleRenameSubmit} style={{ display: "flex", flexDirection: "column", gap: "16px", marginTop: "8px" }}>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("New Name")}</label>
                  <input style={styles.input} required autoFocus
                    value={renameName} onChange={(e) => setRenameName(e.target.value)} />
                </div>
                <div style={styles.modalActions}>
                  <button type="button" style={styles.secondaryButton} onClick={() => setShowRenameModal(false)}>{t("Cancel")}</button>
                  <button type="submit" style={styles.primaryButton}>{t("Save")}</button>
                </div>
              </form>
            </div>
          </div>
        )}

        {contextMenu && (
          <div 
            className="context-menu-box" 
            style={{...styles.contextMenuBox, top: contextMenu.y, left: contextMenu.x}}
            onClick={(e) => e.stopPropagation()}
          >
            {contextMenu.targetId === null ? (
              <>
                <div style={styles.contextMenuItem} onClick={async () => { closeMenu(); setIsLoading(true); await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e)); await fetchDirectory(); }}>{t("Refresh")}</div>
                <div style={styles.contextMenuItem} onClick={() => { setShowFolderModal(true); closeMenu(); }}>{t("New Folder")}</div>
                <div style={styles.contextMenuItem} onClick={() => { handleUpload(); closeMenu(); }}>{t("Upload File")}</div>
                <div 
                  style={{...styles.contextMenuItem, opacity: clipboard ? 1 : 0.4, cursor: clipboard ? "pointer" : "default"}} 
                  onClick={() => { if(clipboard) triggerPaste(); closeMenu(); }}
                >
                  {t("Paste")}
                </div>
                <div style={styles.contextMenuItem} onClick={() => { setSelectedNodes(new Set(nodes.map(n => n.id))); closeMenu(); }}>{t("Select All")}</div>
              </>
            ) : (
              <>
                <div style={styles.contextMenuItem} onClick={() => { triggerCut(Array.from(selectedNodes)); closeMenu(); }}>{t("Cut")}</div>
                <div style={styles.contextMenuItem} onClick={(e) => { triggerShare(e as any, contextMenu.targetId!); closeMenu(); }}>{t("Share")}</div>
                <div style={styles.contextMenuItem} onClick={() => {
                  const targetNode = nodes.find(n => n.id === contextMenu.targetId);
                  setRenameTargetId(contextMenu.targetId);
                  setRenameName(targetNode?.name || "");
                  setShowRenameModal(true);
                  closeMenu();
                }}>{t("Rename")}</div>
                <div style={styles.contextMenuItem} onClick={async () => {
                  const selectedFiles = Array.from(selectedNodes).map(id => nodes.find(n => n.id === id)).filter(Boolean);
                  if (selectedFiles.length === 0) { closeMenu(); return; }

                  const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
                  let dir = "";

                  if (config.useDefaultDownloadPath && config.downloadPath) {
                    dir = config.downloadPath;
                  } else {
                    const selected = await open({ directory: true, title: "Select Download Folder" });
                    if (!selected) { closeMenu(); return; }
                    dir = selected as string;
                  }

                  const activeDown = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");
                  const sep = dir.includes('\\') ? '\\' : '/';

                  for (const file of selectedFiles) {
                      if (file.node_type === "Directory") {
                      const flatFiles = await invoke<any[]>("vfs_get_folder_tree", { id: file.id });
                      if (flatFiles.length === 0) continue;

                      const groupId = "g_" + Date.now() + Math.random().toString(36).substr(2, 5);
                      activeDown.push({ id: groupId, isGroup: true, name: file.name, status: "Queued", totalItems: flatFiles.length, finishedItems: 0 });

                      flatFiles.forEach(f => {
                          let totalSize = 0;
                          if (f.size) {
                          const match = f.size.match(/([\d.]+)\s*([KMG]?)/i);
                          if (match) {
                              let val = parseFloat(match[1]);
                              if (match[2].toUpperCase() === 'K') val *= 1024;
                              if (match[2].toUpperCase() === 'M') val *= 1024 * 1024;
                              if (match[2].toUpperCase() === 'G') val *= 1024 * 1024 * 1024;
                              totalSize = Math.round(val);
                          }
                          }
                          activeDown.push({
                          id: "t_" + Date.now() + Math.random().toString(36).substr(2, 5),
                          groupId, isGroup: false, name: f.name, type: "Download", status: "Queued",
                          vfsId: f.id, localPath: `${dir}${sep}${f.rel_path.split('/').join(sep)}`, resumeOffset: 0, totalSize
                          });
                      });
                      } else {
                      let totalSize = 0;
                      if (file.size) {
                          const match = file.size.match(/([\d.]+)\s*([KMG]?)/i);
                          if (match) {
                          let val = parseFloat(match[1]);
                          if (match[2].toUpperCase() === 'K') val *= 1024;
                          if (match[2].toUpperCase() === 'M') val *= 1024 * 1024;
                          if (match[2].toUpperCase() === 'G') val *= 1024 * 1024 * 1024;
                          totalSize = Math.round(val);
                          }
                      }
                      activeDown.push({
                          id: Date.now().toString() + Math.random().toString(36).substr(2, 5),
                          isGroup: false, name: file.name, type: "Download", status: "Queued",
                          vfsId: file.id, localPath: `${dir}${sep}${file.name}`, resumeOffset: 0, totalSize
                      });
                      }
                  }

                  localStorage.setItem("heriheri_down_active", JSON.stringify(activeDown));
                  window.dispatchEvent(new CustomEvent("DOWN_TASK_START"));
                  showAlert("Download Queued", `Items added to the queue.`);
                  closeMenu();
                }}>{t("Download")}</div>
                <div style={{...styles.contextMenuItem, color: "#ef4444"}} onClick={() => { triggerBatchDelete(); closeMenu(); }}>{t("Delete")}</div>
              </>
            )}
          </div>
        )}
      </div>
    </DndContext>
  );
}

const styles: { [key: string]: React.CSSProperties } = {
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "20px" },
  headerActions: { display: "flex", gap: "12px", alignItems: "center" },
  breadcrumbBar: { display: "flex", alignItems: "center", fontSize: "12px", color: "#111827", backgroundColor: "#f3f4f6", padding: "8px 12px", borderRadius: "0", border: "1px solid #111827", fontWeight: "600" },
  breadcrumbLink: { cursor: "pointer", color: "#111827", textDecoration: "underline" },
  listContainer: { backgroundColor: "#ffffff", borderRadius: "0", border: "1px solid #111827", display: "flex", flexDirection: "column", flex: 1, overflow: "hidden", position: "relative", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)" },
  listHeaderRow: { display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 140px 140px 100px", padding: "12px 20px", backgroundColor: "#f3f4f6", borderBottom: "1px solid #111827", fontWeight: "700", color: "#111827", fontSize: "11px", alignItems: "center", textTransform: "uppercase", letterSpacing: "1px" },
  listBody: { overflowY: "auto", flex: 1 },
  listRow: { display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 140px 140px 100px", padding: "10px 20px", borderBottom: "1px solid #e5e7eb", alignItems: "center", cursor: "pointer", transition: "background-color 0.1s, opacity 0.2s", userSelect: "none" },
  cellName: { display: "flex", alignItems: "center", gap: "12px", overflow: "hidden" },
  cellDefault: { fontSize: "13px", color: "#4b5563", whiteSpace: "nowrap" },
  cellActions: { display: "flex", gap: "8px", justifyContent: "flex-end" },
  icon: { width: "16px", height: "16px", objectFit: "contain", flexShrink: 0, display: "block" },
  itemName: { fontSize: "13px", fontWeight: "500", color: "#111827", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
  actionBtn: { display: "flex", alignItems: "center", justifyContent: "center", background: "transparent", border: "1px solid #d1d5db", cursor: "pointer", padding: "6px", borderRadius: "0", color: "#111827", transition: "background 0.2s, border-color 0.2s" },
  statusState: { textAlign: "center", padding: "48px", color: "#111827", fontSize: "12px", fontWeight: "600", textTransform: "uppercase", letterSpacing: "1px" },
  primaryButton: { backgroundColor: "#111827", color: "#ffffff", padding: "8px 16px", borderRadius: "0", border: "1px solid #111827", fontSize: "12px", fontWeight: "600", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px" },
  secondaryButton: { backgroundColor: "#ffffff", color: "#111827", padding: "8px 16px", borderRadius: "0", border: "1px solid #111827", fontSize: "12px", fontWeight: "600", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px" },
  dangerButton: { backgroundColor: "#ffffff", color: "#ef4444", padding: "8px 16px", borderRadius: "0", border: "1px solid #ef4444", fontSize: "12px", fontWeight: "600", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px" },
  modalOverlay: { position: "fixed", top: 0, left: 0, right: 0, bottom: 0, backgroundColor: "rgba(255, 255, 255, 0.9)", display: "flex", justifyContent: "center", alignItems: "center", zIndex: 999 },
  modalBox: { backgroundColor: "#ffffff", padding: "32px", borderRadius: "0", border: "2px solid #111827", width: "360px", boxShadow: "8px 8px 0px 0px rgba(17, 24, 39, 1)", display: "flex", flexDirection: "column", gap: "24px" },
  modalTitle: { margin: 0, fontSize: "16px", fontWeight: "800", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  modalText: { margin: 0, fontSize: "13px", color: "#4b5563", lineHeight: "1.5" },
  modalActions: { display: "flex", justifyContent: "flex-end", gap: "12px", marginTop: "8px" },
  inputGroup: { display: "flex", flexDirection: "column", gap: "8px" },
  inputLabel: { fontSize: "10px", fontWeight: "700", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  input: { padding: "10px 12px", backgroundColor: "#ffffff", color: "#111827", border: "1px solid #111827", borderRadius: "0", fontSize: "13px", outline: "none" },
  readOnlyInput: { padding: "10px 12px", backgroundColor: "#f3f4f6", border: "1px solid #111827", borderRadius: "0", fontSize: "13px", color: "#111827", outline: "none", cursor: "text" },
  contextMenuBox: { position: "fixed", backgroundColor: "#ffffff", border: "1px solid #111827", borderRadius: "0", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)", padding: "4px", minWidth: "160px", zIndex: 9999, display: "flex", flexDirection: "column", gap: "2px" },
  contextMenuItem: { padding: "10px 12px", fontSize: "11px", fontWeight: "700", color: "#111827", cursor: "pointer", borderRadius: "0", textTransform: "uppercase", letterSpacing: "0.5px" },
};