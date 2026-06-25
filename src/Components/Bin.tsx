import { useState, useEffect, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getFileIcon } from "../Utils/fileIcons";

import { useFileSelection } from "../Hooks/useFileSelection";
import { useRectangleSelect } from "../Hooks/useRectangleSelect";
import { useContextMenu } from "../Hooks/useContextMenu";
import { useTranslation } from "react-i18next";

export default function Bin({ onBack }: { onBack: () => void }) {
  const { t } = useTranslation();
  const [nodes, setNodes] = useState<any[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const [alertData, setAlertData] = useState<{ title: string, msg: string } | null>(null);
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [showEmptyBinModal, setShowEmptyBinModal] = useState(false);
  const [actionTarget, setActionTarget] = useState<number | null>(null);

  const [expandedFolders, setExpandedFolders] = useState<Record<number, boolean>>({});

  const visibleNodes = useMemo(() => {
    let visible: any[] = [];
    const topLevel = nodes.filter(n => !nodes.some(p => p.id === n.pid));
    
    const addNodes = (list: any[], depth: number) => {
      list.forEach(node => {
        visible.push({ ...node, depth });
        if (expandedFolders[node.id]) {
          addNodes(nodes.filter(n => n.pid === node.id), depth + 1);
        }
      });
    };
    
    addNodes(topLevel, 0);
    return visible;
  }, [nodes, expandedFolders]);

  const triggerBatchDelete = () => {
    setActionTarget(null);
    setShowDeleteModal(true);
  };
  
  const { selectedNodes, setSelectedNodes, handleRowClick, clearSelection } = useFileSelection(visibleNodes, triggerBatchDelete, () => {}, () => {});
  const selectionBox = useRectangleSelect(containerRef, visibleNodes, setSelectedNodes);
  const { contextMenu, handleContextMenu, closeMenu } = useContextMenu();

  useEffect(() => {
    const initBin = async () => {
      setIsLoading(true);
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await fetchBin();
    };
    initBin();
  }, []);

  const showAlert = (title: string, msg: string) => setAlertData({ title, msg });

  async function fetchBin() {
    setIsLoading(true);
    clearSelection();
    try {
      const data = await invoke<any[]>("vfs_list_bin");
      setNodes(data);
    } catch (error) {
      console.error("Bin fetch error:", error);
    } finally {
      setIsLoading(false);
    }
  }

  async function restoreItems(ids: number[]) {
    if (ids.length === 0) return;
    setIsLoading(true);
    try {
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await invoke("vfs_restore_items", { ids });
      await invoke("vfs_sync_push").catch((e) => console.warn("Sync push skipped:", e));
      
      clearSelection();
      fetchBin();
      window.dispatchEvent(new CustomEvent("TASK_END"));
    } catch (err) {
      showAlert("Restore Error", String(err));
      setIsLoading(false);
    }
  }

  async function hardDeleteItems(ids: number[]) {
    if (ids.length === 0) return;
    setIsLoading(true);
    setShowDeleteModal(false);
    setShowEmptyBinModal(false);
    try {
      await invoke("vfs_sync_pull").catch((e) => console.warn("Sync pull skipped:", e));
      await invoke("vfs_hard_delete_items", { ids });
      await invoke("vfs_sync_push").catch((e) => console.warn("Sync push skipped:", e));
      
      clearSelection();
      fetchBin();
    } catch (err) {
      showAlert("Delete Error", String(err));
      setIsLoading(false);
    }
  }

  function formatTime(unixSeconds: number) {
    if (!unixSeconds || unixSeconds === 0) return "-";
    const d = new Date(unixSeconds * 1000);
    return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')} ${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}`;
  }

  const idsToDelete = actionTarget !== null ? [actionTarget] : Array.from(selectedNodes);

  return (
    <div 
      style={{ display: "flex", flexDirection: "column", height: "100%", position: "relative" }} 
      onClick={clearSelection}
      onContextMenu={(e) => handleContextMenu(e, null)}
    >
      <header style={styles.header}>
        <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: "16px" }}>
            <button style={{...styles.secondaryButton, padding: "6px 10px", display: "flex", alignItems: "center"}} onClick={onBack}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>
            </button>
            <h2 style={{ margin: 0, textTransform: "uppercase", letterSpacing: "1px", fontSize: "20px" }}>{t("Recycle Bin")}</h2>
          </div>
        </div>
        <div style={styles.headerActions}></div>
      </header>

      <div style={styles.listContainer} ref={containerRef}>
        <div style={styles.listHeaderRow}>
          <div style={styles.cellName}>{t("Name")}</div>
          <div style={styles.cellDefault}>{t("Size")}</div>
          <div style={styles.cellDefault}>{t("Deleted Time")}</div>
          <div style={styles.cellDefault}>{t("MD5 Hash")}</div>
          <div style={styles.cellActions}>{t("Actions")}</div>
        </div>

        {isLoading && <div style={styles.statusState}>{t("SYNCING & LOADING...")}</div>}
        {!isLoading && nodes.length === 0 && (
          <div style={styles.statusState}>{t("The Recycle Bin is empty.")}</div>
        )}

        {!isLoading && nodes.length > 0 && (
          <div style={styles.listBody}>
            {visibleNodes.map((node, index) => {
              const isDir = node.node_type === "Directory";
              const isSelected = selectedNodes.has(node.id);
              const isExpanded = expandedFolders[node.id];
              const hasChildren = nodes.some(n => n.pid === node.id);
              
              return (
                <div 
                  key={node.id} 
                  className="file-row"
                  onClick={(e) => handleRowClick(e, index, node.id)}
                  onContextMenu={(e) => {
                    if (!isSelected) setSelectedNodes(new Set([node.id]));
                    handleContextMenu(e, node.id);
                  }}
                  style={{
                    ...styles.listRow, 
                    backgroundColor: isSelected ? "#e5e7eb" : "transparent",
                  }}
                >
                  <div style={{ ...styles.cellName, paddingLeft: `${node.depth * 24}px` }}>
                    {isDir && hasChildren ? (
                      <span 
                        style={{ cursor: "pointer", marginRight: "8px", display: "flex", alignItems: "center" }}
                        onClick={(e) => {
                          e.stopPropagation();
                          setExpandedFolders({...expandedFolders, [node.id]: !isExpanded});
                        }}
                      >
                        {isExpanded ? (
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M6 9l6 6 6-6"/></svg>
                        ) : (
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M9 18l6-6-6-6"/></svg>
                        )}
                      </span>
                    ) : (
                      <span style={{ width: "22px", display: "inline-block" }}></span>
                    )}
                    <img src={getFileIcon(node.name, isDir)} alt="icon" style={styles.icon} />
                    <span style={{...styles.itemName, color: "#111827"}}>{node.name}</span>
                  </div>
                  <div style={styles.cellDefault}>{isDir ? "-" : node.size}</div>
                  <div style={styles.cellDefault}>{formatTime(node.time)}</div>
                  <div style={{...styles.cellDefault, fontSize: "11px", fontFamily: "monospace"}}>{isDir ? "-" : node.md5.substring(0, 16) + "..."}</div>
                  <div style={styles.cellActions}>
                    <button style={styles.actionBtn} title="Restore" onMouseDown={(e) => e.stopPropagation()} onClick={(e) => {
                      e.stopPropagation();
                      restoreItems([node.id]);
                    }}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/></svg>
                    </button>
                    <button style={{...styles.actionBtn, color: "#ef4444", borderColor: "#ef4444"}} title="Delete" onMouseDown={(e) => e.stopPropagation()} onClick={(e) => {
                      e.stopPropagation();
                      setActionTarget(node.id);
                      setShowDeleteModal(true);
                    }}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {/* RECTANGLE SELECTION OVERLAY DIV */}
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

      {showDeleteModal && (
        <div style={styles.modalOverlay} onClick={() => setShowDeleteModal(false)}>
          <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
            <h3 style={styles.modalTitle}>{t("Delete Permanently?")}</h3>
            <p style={styles.modalText}>
              {t("Are you sure you want to permanently delete ")}<strong>{idsToDelete.length} {t("item(s)")}</strong>{t("? This action cannot be undone and files cannot be recovered.")}
            </p>
            <div style={styles.modalActions}>
              <button style={styles.secondaryButton} onClick={() => setShowDeleteModal(false)}>{t("Cancel")}</button>
              <button style={styles.dangerButton} onClick={() => hardDeleteItems(idsToDelete)}>{t("Delete")}</button>
            </div>
          </div>
        </div>
      )}

      {showEmptyBinModal && (
        <div style={styles.modalOverlay} onClick={() => setShowEmptyBinModal(false)}>
          <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
            <h3 style={styles.modalTitle}>{t("Empty Recycle Bin?")}</h3>
            <p style={styles.modalText}>
              {t("Are you sure you want to permanently delete ")}<strong>{t("all")} {nodes.length} {t("items")}</strong> {t("in the Recycle Bin?")}
            </p>
            <div style={styles.modalActions}>
              <button style={styles.secondaryButton} onClick={() => setShowEmptyBinModal(false)}>{t("Cancel")}</button>
              <button style={styles.dangerButton} onClick={() => hardDeleteItems(nodes.map(n => n.id))}>{t("Empty Bin")}</button>
            </div>
          </div>
        </div>
      )}

      {/* --- CONTEXT MENU OVERLAY --- */}
      {contextMenu && (
        <div 
          className="context-menu-box" 
          style={{...styles.contextMenuBox, top: contextMenu.y, left: contextMenu.x}}
          onClick={(e) => e.stopPropagation()} 
        >
          {contextMenu.targetId === null ? (
            <>
              <div style={styles.contextMenuItem} onClick={async () => { closeMenu(); setIsLoading(true); await invoke("vfs_sync_pull").catch((err) => console.warn("Sync pull skipped:", err)); await fetchBin(); }}>{t("Refresh")}</div>
              <div style={styles.contextMenuItem} onClick={() => { setShowEmptyBinModal(true); closeMenu(); }}>{t("Empty Bin")}</div>
              <div style={styles.contextMenuItem} onClick={() => { setSelectedNodes(new Set(visibleNodes.map(n => n.id))); closeMenu(); }}>{t("Select All")}</div>
            </>
          ) : (
            <>
              <div style={styles.contextMenuItem} onClick={() => { restoreItems(Array.from(selectedNodes)); closeMenu(); }}>{t("Restore")}</div>
              <div style={{...styles.contextMenuItem, color: "#ef4444"}} onClick={() => { triggerBatchDelete(); closeMenu(); }}>{t("Delete")}</div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// --------------------------------------------------------
// Styling (Matches Neo-Brutalist Home.tsx exactly)
// --------------------------------------------------------
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
  actionBtn: { display: "flex", alignItems: "center", justifyContent: "center", background: "transparent", border: "1px solid #d1d5db", cursor: "pointer", padding: "6px", borderRadius: "0", color: "#111827", fontWeight: "600", textTransform: "uppercase", transition: "background 0.2s, border-color 0.2s" },
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