import { useState, useRef } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { getFileIcon } from "../Utils/fileIcons";

import { useFileSelection } from "../Hooks/useFileSelection";
import { useRectangleSelect } from "../Hooks/useRectangleSelect";
import { useContextMenu } from "../Hooks/useContextMenu";

function formatBytes(sizeStr: string) {
  if (!sizeStr || sizeStr === "-") return "-";
  if (/[a-zA-Z]/.test(sizeStr)) return sizeStr;

  const bytes = parseInt(sizeStr, 10);
  if (isNaN(bytes)) return sizeStr;
  if (bytes === 0) return "0 B";

  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

function TransferRowNode({ node, index, isSelected, handleRowClick, handleContextMenu, setSelectedNodes }: any) {
  return (
    <div 
      style={{...styles.listRow, backgroundColor: isSelected ? "#e5e7eb" : "transparent"}}
      onClick={(e) => handleRowClick(e, index, node.id)}
      onContextMenu={(e) => {
        if (!isSelected) setSelectedNodes(new Set([node.id]));
        handleContextMenu(e, node.id);
      }}
    >
      <div style={styles.cellName}>
        <img src={getFileIcon(node.name, false)} alt="icon" style={styles.icon} />
        <span style={{...styles.itemName, color: "#111827"}}>{node.name}</span>
      </div>
      <div style={styles.cellDefault}>{formatBytes(node.size)}</div>
      <div style={styles.cellDefault}>{node.chunks}</div>
      <div style={{...styles.cellDefault, fontSize: "11px", fontFamily: "monospace"}}>{node.md5.substring(0, 16)}...</div>
    </div>
  );
}

export default function Transfer() {
  const { t } = useTranslation();
  const [shareCode, setShareCode] = useState("");
  const [isResolving, setIsResolving] = useState(false);
  
  // --- MINIMUM FIX: Array state for batch resolution ---
  const [resolvedNodes, setResolvedNodes] = useState<any[]>([]);
  const containerRef = useRef<HTMLDivElement>(null);

  // --- MINIMUM FIX: Integrate selection hooks ---
  const { selectedNodes, setSelectedNodes, handleRowClick, clearSelection } = useFileSelection(resolvedNodes, () => {}, () => {}, () => {});
  const selectionBox = useRectangleSelect(containerRef, resolvedNodes, setSelectedNodes);
  const { contextMenu, handleContextMenu, closeMenu } = useContextMenu();

  const [alertData, setAlertData] = useState<{ title: string, msg: string } | null>(null);
  const showAlert = (title: string, msg: string) => setAlertData({ title, msg });

  const [showRentModal, setShowRentModal] = useState(false);
  const [rentFolders, setRentFolders] = useState<any[]>([]);
  const [rentBreadcrumbs, setRentBreadcrumbs] = useState<{id: number, name: string}[]>([]);

  const fetchRentDir = async () => {
    try {
      const data = await invoke<any[]>("vfs_list_dir");
      setRentFolders(data.filter((n: any) => n.node_type === "Directory"));
      const crumbs = await invoke<{id: number, name: string}[]>("vfs_get_breadcrumbs").catch(() => []);
      setRentBreadcrumbs(crumbs);
    } catch (err) {
      console.error("Failed to fetch rent directory:", err);
    }
  };

  const handleRentClick = () => {
    if (selectedNodes.size === 0) return;
    setShowRentModal(true);
    fetchRentDir();
  };

  const confirmRent = async () => {
    setShowRentModal(false);
    try {
      const currentPid = await invoke<number>("vfs_get_current_pid").catch(() => 0);
      await invoke("vfs_sync_pull").catch(() => {});
      
      for (const id of Array.from(selectedNodes)) {
         const node = resolvedNodes.find(n => n.id === id);
         if (node) await invoke("vfs_rent_item", { code: node.shareCode, targetPid: currentPid });
      }
      
      await invoke("vfs_sync_push").catch(() => {});
      showAlert(t("Success"), t("Rented to your cloud successfully!"));
      clearSelection();
      window.dispatchEvent(new CustomEvent("TASK_END"));
    } catch (err) {
      showAlert(t("Rent Error"), String(err));
    }
  };

  const handleResolve = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!shareCode.trim()) return;
    
    setIsResolving(true);
    try {
      const codes = shareCode.split('\n').map(c => c.trim()).filter(c => c.startsWith("heri://"));
      let results = [];
      
      for (let i = 0; i < codes.length; i++) {
        const data = await invoke<any>("vfs_resolve_share_code", { code: codes[i] });
        results.push({
          id: i + 1, // Generate mock ID for UI selection
          name: data.name,
          size: data.size,
          md5: data.md5,
          chunks: data.chunks,
          shareCode: codes[i]
        });
      }
      setResolvedNodes(results);
      clearSelection();
    } catch (err) {
      showAlert(t("Resolution Error"), String(err));
    } finally {
      setIsResolving(false);
    }
  };

  const handleDownload = async () => {
    if (selectedNodes.size === 0) return;
    try {
      const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
      let dir = "";
      
      if (config.useDefaultDownloadPath && config.downloadPath) {
        dir = config.downloadPath;
      } else {
        const selected = await open({ directory: true, title: "Select Download Folder" });
        if (!selected) return;
        dir = selected as string;
      }

      const activeDown = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");
      const sep = dir.includes('\\') ? '\\' : '/';
      
      for (const id of Array.from(selectedNodes)) {
         const node = resolvedNodes.find(n => n.id === id);
         if (!node) continue;
         
         let totalSize = 0;
         if (node.size) {
             const match = node.size.match(/([\d.]+)\s*([KMG]?)/i);
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
             isGroup: false, 
             name: node.name, 
             type: "Download", 
             status: "Queued",
             vfsId: 0,
             shareCode: node.shareCode,
             localPath: `${dir}${sep}${node.name}`, 
             resumeOffset: 0, 
             totalSize
         });
      }

      localStorage.setItem("heriheri_down_active", JSON.stringify(activeDown));
      window.dispatchEvent(new CustomEvent("DOWN_TASK_START"));
      showAlert(t("Download Queued"), t("Added to Download Queue!"));
      clearSelection();
    } catch(err) {
       showAlert(t("Download Error"), String(err));
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }} onClick={clearSelection} onContextMenu={(e) => handleContextMenu(e, null)}>
      <header style={styles.header}>
        <div style={{ display: "flex", gap: "24px", alignItems: "center" }}>
          <h2 style={styles.tabTitle}>{t("Transfer & Rent")}</h2>
        </div>
      </header>

      <div style={styles.container}>
        <div style={styles.scrollArea}>
          <div style={styles.section}>
            <h3 style={styles.sectionTitle}>{t("Resolve HeriHeri Share Codes")}</h3>
            <p style={styles.helperText}>
              {t("Paste one or more heri:// codes below (one per line).")}
            </p>
            
            <form onSubmit={handleResolve} style={{ display: "flex", gap: "12px", marginTop: "8px", alignItems: "flex-start" }}>
              <textarea 
                style={{...styles.input, flex: 1, fontSize: "12px", fontFamily: "monospace", minHeight: "80px", resize: "vertical", whiteSpace: "pre"}} 
                placeholder="heri://..."
                value={shareCode}
                onChange={(e) => setShareCode(e.target.value)}
              />
              <button 
                type="submit" 
                style={{...styles.primaryButton, height: "80px"}}
                disabled={isResolving || !shareCode}
              >
                {isResolving ? t("Resolving...") : t("Resolve")}
              </button>
            </form>
          </div>

          {resolvedNodes.length > 0 && (
            <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
              
              {/* --- MINIMUM FIX: Unified Single Warning Box --- */}
              <div style={styles.warningBox}>
                <strong>{t("RENTAL WARNING:")}</strong> {t("If you choose to Rent, this file will appear in your cloud, but you do not own it. If the original uploader deletes it, your rented link will break. To own it permanently, Download it and re-upload it to your account.")}
              </div>

              {/* --- MINIMUM FIX: List Container mimicking Home.tsx --- */}
              <div style={{...styles.listContainer, maxHeight: "400px"}} ref={containerRef}>
                <div style={styles.listHeaderRow}>
                  <div style={styles.cellName}>{t("Name")}</div>
                  <div style={styles.cellDefault}>{t("Size")}</div>
                  <div style={styles.cellDefault}>{t("Chunks")}</div>
                  <div style={styles.cellDefault}>{t("MD5 Hash")}</div>
                </div>

                <div style={styles.listBody}>
                  {resolvedNodes.map((node, index) => (
                    <TransferRowNode
                      key={node.id}
                      node={node}
                      index={index}
                      isSelected={selectedNodes.has(node.id)}
                      handleRowClick={handleRowClick}
                      setSelectedNodes={setSelectedNodes}
                      handleContextMenu={handleContextMenu}
                    />
                  ))}
                </div>

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
            </div>
          )}
        </div>
      </div>

      {contextMenu && contextMenu.targetId !== null && (
        <div 
          className="context-menu-box" 
          style={{...styles.contextMenuBox, top: contextMenu.y, left: contextMenu.x}}
          onClick={(e) => e.stopPropagation()}
        >
          <div style={styles.contextMenuItem} onClick={() => { handleRentClick(); closeMenu(); }}>{t("Rent to My Cloud")}</div>
          <div style={styles.contextMenuItem} onClick={() => { handleDownload(); closeMenu(); }}>{t("Download Directly")}</div>
          <div style={styles.contextMenuItem} onClick={() => { setSelectedNodes(new Set(resolvedNodes.map(n => n.id))); closeMenu(); }}>{t("Select All")}</div>
        </div>
      )}

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

      {showRentModal && (
        <div style={styles.modalOverlay} onClick={() => setShowRentModal(false)}>
          <div style={{...styles.modalBox, width: "480px"}} onClick={(e) => e.stopPropagation()}>
            <h3 style={styles.modalTitle}>{t("Select Cloud Destination")}</h3>
            
            {/* Breadcrumb Navigator */}
            <div style={{ display: "flex", gap: "8px", alignItems: "center", backgroundColor: "#f3f4f6", padding: "8px 12px", border: "2px solid #111827", fontSize: "12px", fontWeight: "700", overflowX: "auto", textTransform: "uppercase" }}>
              {rentBreadcrumbs.map((crumb, idx) => (
                 <span key={crumb.id} style={{ display: "flex", alignItems: "center" }}>
                   <span 
                     style={{ cursor: "pointer", textDecoration: "underline", color: "#111827" }} 
                     onClick={async () => { await invoke("vfs_enter_folder", { id: crumb.id }); fetchRentDir(); }}
                   >
                     {crumb.id === 0 || crumb.name === "All Files" ? t("All Files") : crumb.name}
                   </span>
                   {idx < rentBreadcrumbs.length - 1 && <span style={{ margin: "0 6px", color: "#9ca3af" }}>/</span>}
                 </span>
              ))}
            </div>

            {/* Folder List */}
            <div style={{ border: "2px solid #111827", height: "240px", overflowY: "auto", display: "flex", flexDirection: "column", backgroundColor: "#ffffff" }}>
               {rentFolders.length === 0 && (
                 <div style={{ padding: "32px", textAlign: "center", color: "#4b5563", fontSize: "12px", fontWeight: "600", textTransform: "uppercase", letterSpacing: "1px" }}>
                   {t("No subfolders")}
                 </div>
               )}
               {rentFolders.map(folder => (
                  <div 
                    key={folder.id} 
                    style={{ padding: "10px 16px", borderBottom: "1px solid #e5e7eb", display: "flex", alignItems: "center", gap: "12px", cursor: "pointer", transition: "background-color 0.1s" }}
                    onMouseOver={(e) => e.currentTarget.style.backgroundColor = "#f3f4f6"}
                    onMouseOut={(e) => e.currentTarget.style.backgroundColor = "transparent"}
                    onDoubleClick={async () => { await invoke("vfs_enter_folder", { id: folder.id }); fetchRentDir(); }}
                    title={t("Double-click to enter")}
                  >
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#111827" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
                    <span style={{ fontSize: "13px", fontWeight: "700", color: "#111827" }}>{folder.name}</span>
                  </div>
               ))}
            </div>

            <div style={styles.modalActions}>
              <button style={styles.secondaryButton} onClick={() => setShowRentModal(false)}>{t("Cancel")}</button>
              <button style={styles.primaryButton} onClick={confirmRent}>{t("Rent Here")}</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// --------------------------------------------------------
// Styling
// --------------------------------------------------------
const styles: { [key: string]: React.CSSProperties } = {
  header: { display: "flex", justifyContent: "space-between", alignItems: "flex-end", marginBottom: "20px", borderBottom: "2px solid #111827" },
  tabTitle: { margin: 0, paddingBottom: "12px", color: "#111827", borderBottom: "2px solid #111827", fontSize: "18px", fontWeight: "800", textTransform: "uppercase", letterSpacing: "1px" },
  container: { backgroundColor: "#ffffff", borderRadius: "0", border: "1px solid #111827", display: "flex", flexDirection: "column", flex: 1, overflow: "hidden", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)" },
  scrollArea: { padding: "32px", overflowY: "auto", flex: 1 },
  section: { display: "flex", flexDirection: "column", gap: "8px", marginBottom: "24px" },
  sectionTitle: { margin: "0", fontSize: "14px", fontWeight: "800", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  helperText: { margin: 0, fontSize: "13px", color: "#4b5563" },
  input: { padding: "12px 16px", backgroundColor: "#f9fafb", color: "#111827", border: "2px solid #111827", borderRadius: "0", fontSize: "13px", outline: "none", fontWeight: "600", transition: "border-color 0.2s" },
  primaryButton: { display: "flex", alignItems: "center", justifyContent: "center", backgroundColor: "#111827", color: "#ffffff", padding: "12px 24px", borderRadius: "0", border: "2px solid #111827", fontSize: "13px", fontWeight: "700", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px", transition: "background 0.2s", whiteSpace: "nowrap" },
  secondaryButton: { display: "flex", alignItems: "center", justifyContent: "center", backgroundColor: "#ffffff", color: "#111827", padding: "12px 24px", borderRadius: "0", border: "2px solid #111827", fontSize: "13px", fontWeight: "700", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px", transition: "background 0.2s", whiteSpace: "nowrap" },
  warningBox: { padding: "16px", backgroundColor: "#fef2f2", border: "1px solid #ef4444", color: "#991b1b", fontSize: "12px", lineHeight: "1.5" },
  
  // Reused Home.tsx List Styles
  listContainer: { backgroundColor: "#ffffff", borderRadius: "0", border: "1px solid #111827", display: "flex", flexDirection: "column", flex: 1, overflow: "hidden", position: "relative", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)" },
  listHeaderRow: { display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 100px 140px", padding: "12px 20px", backgroundColor: "#f3f4f6", borderBottom: "1px solid #111827", fontWeight: "700", color: "#111827", fontSize: "11px", alignItems: "center", textTransform: "uppercase", letterSpacing: "1px" },
  listBody: { overflowY: "auto", flex: 1 },
  listRow: { display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 100px 140px", padding: "10px 20px", borderBottom: "1px solid #e5e7eb", alignItems: "center", cursor: "pointer", transition: "background-color 0.1s", userSelect: "none" },
  cellName: { display: "flex", alignItems: "center", gap: "12px", overflow: "hidden" },
  cellDefault: { fontSize: "13px", color: "#4b5563", whiteSpace: "nowrap" },
  icon: { width: "16px", height: "16px", objectFit: "contain", flexShrink: 0, display: "block" },
  itemName: { fontSize: "13px", fontWeight: "500", color: "#111827", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
  
  modalOverlay: { position: "fixed", top: 0, left: 0, right: 0, bottom: 0, backgroundColor: "rgba(255, 255, 255, 0.9)", display: "flex", justifyContent: "center", alignItems: "center", zIndex: 999 },
  modalBox: { backgroundColor: "#ffffff", padding: "32px", borderRadius: "0", border: "2px solid #111827", width: "360px", boxShadow: "8px 8px 0px 0px rgba(17, 24, 39, 1)", display: "flex", flexDirection: "column", gap: "24px" },
  modalTitle: { margin: 0, fontSize: "16px", fontWeight: "800", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  modalText: { margin: 0, fontSize: "13px", color: "#4b5563", lineHeight: "1.5" },
  modalActions: { display: "flex", justifyContent: "flex-end", gap: "12px", marginTop: "8px" },
  contextMenuBox: { position: "fixed", backgroundColor: "#ffffff", border: "1px solid #111827", borderRadius: "0", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)", padding: "4px", minWidth: "160px", zIndex: 9999, display: "flex", flexDirection: "column", gap: "2px" },
  contextMenuItem: { padding: "10px 12px", fontSize: "11px", fontWeight: "700", color: "#111827", cursor: "pointer", borderRadius: "0", textTransform: "uppercase", letterSpacing: "0.5px" },
};