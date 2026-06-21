import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

const globalDownProgress: Record<string, any> = {};
const globalDownLastUpdate: Record<string, { time: number, loaded: number, speed: string, eta: string }> = {};

listen("download_progress", (event: any) => {
  const { task_id, loaded, total } = event.payload;
  const now = Date.now();

  if (!globalDownLastUpdate[task_id]) {
    globalDownLastUpdate[task_id] = { time: now, loaded: loaded, speed: "Calculating...", eta: "--" };
  }

  const last = globalDownLastUpdate[task_id];
  const timeDiff = (now - last.time) / 1000;

  if (timeDiff >= 0.5 && loaded > last.loaded) {
    const bytesPerSec = (loaded - last.loaded) / timeDiff;
    const speedStr = (bytesPerSec / 1024 / 1024).toFixed(2) + " MB/s";
    
    const remainingBytes = total - loaded;
    const etaSeconds = Math.round(remainingBytes / bytesPerSec);
    const etaStr = etaSeconds > 60 ? `${Math.floor(etaSeconds/60)}m ${etaSeconds%60}s` : `${etaSeconds}s`;

    globalDownLastUpdate[task_id] = { time: now, loaded, speed: speedStr, eta: etaStr };
  }

  const percent = total === 0 ? 0 : Math.round((loaded / total) * 100);
  
  globalDownProgress[task_id] = { percent, speed: globalDownLastUpdate[task_id].speed, eta: globalDownLastUpdate[task_id].eta };
  window.dispatchEvent(new CustomEvent("DOWN_PROGRESS_UPDATE"));
});

export default function Downloading() {
  const [activeTasks, setActiveTasks] = useState<any[]>([]);
  const [progressMap, setProgressMap] = useState<Record<string, any>>(globalDownProgress);
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({});

  const loadTasks = () => {
    const active = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");
    setActiveTasks(active);
  };

  useEffect(() => {
    loadTasks();
    window.addEventListener("DOWN_TASK_START", loadTasks);
    window.addEventListener("DOWN_TASK_END", loadTasks);

    const updateUI = () => setProgressMap({ ...globalDownProgress });
    window.addEventListener("DOWN_PROGRESS_UPDATE", updateUI);

    return () => {
      window.removeEventListener("DOWN_TASK_START", loadTasks);
      window.removeEventListener("DOWN_TASK_END", loadTasks);
      window.removeEventListener("DOWN_PROGRESS_UPDATE", updateUI);
    };
  }, []);

  async function handleControl(task: any, action: number) {
    if (action === 2 && !confirm(`Cancel download?`)) return;
    
    let active = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");
    
    // If it's a group, apply control to ALL children
    const targets = task.isGroup ? active.filter((t: any) => t.groupId === task.id) : [task];

    for (const t of targets) {
      if (action === 0) {
        const idx = active.findIndex((x: any) => x.id === t.id);
        if (idx > -1) active[idx].status = "Queued";
      } else if (action === 1) {
        try {
          await invoke("vfs_control_task", { taskId: t.id, action });
          const idx = active.findIndex((x: any) => x.id === t.id);
          if (idx > -1) active[idx].status = "Paused";
        } catch (e) {}
      } else if (action === 2) {
        try {
          await invoke("vfs_control_task", { taskId: t.id, action });
          active = active.filter((x: any) => x.id !== t.id);
        } catch (e) {}
      }
    }

    if (action === 2 && task.isGroup) {
      active = active.filter((x: any) => x.id !== task.id);
    }

    localStorage.setItem("heriheri_down_active", JSON.stringify(active));
    window.dispatchEvent(action === 0 ? new CustomEvent("DOWN_TASK_START") : new CustomEvent("DOWN_TASK_END"));
  }

  // Get Top Level Items (Standalone Files + Groups)
  const topLevelTasks = activeTasks.filter(t => !t.groupId);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      <header style={styles.header}>
        <h2 style={{ margin: 0, textTransform: "uppercase", letterSpacing: "1px", fontSize: "20px", color: "#111827" }}>Downloading</h2>
      </header>

      <div style={styles.listContainer}>
        <div style={styles.listHeaderRow}>
          <div>File Name</div>
          <div>Speed</div>
          <div>ETA</div>
          <div>Progress</div>
          <div style={{ textAlign: "right" }}>Actions</div>
        </div>

        {topLevelTasks.length === 0 ? (
          <div style={styles.statusState}>No active downloads.</div>
        ) : (
          <div style={{ overflowY: "auto", flex: 1 }}>
            {topLevelTasks.map((task) => {
              const isGroup = task.isGroup;
              const p = isGroup ? { percent: Math.round((task.finishedItems / task.totalItems) * 100), speed: "--", eta: "--" } : (progressMap[task.id] || { percent: 0, speed: "Starting...", eta: "--" });
              const children = isGroup ? activeTasks.filter(t => t.groupId === task.id) : [];
              
              const isPaused = isGroup ? children.every(c => c.status === "Paused") : task.status === "Paused";
              const isQueued = isGroup ? children.every(c => c.status === "Queued" || c.status === "Paused") : task.status === "Queued";
              const isExpanded = expandedGroups[task.id];
              
              return (
                <div key={task.id} style={{ borderBottom: "1px solid #111827" }}>
                  <div 
                    style={{ display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 80px 60px 100px", alignItems: "center", padding: "16px 20px", cursor: isGroup ? "pointer" : "default", backgroundColor: isExpanded ? "#f3f4f6" : "transparent" }}
                    onClick={() => { if (isGroup) setExpandedGroups({...expandedGroups, [task.id]: !isExpanded }) }}
                  >
                    <div style={{ display: "flex", alignItems: "center", gap: "12px", overflow: "hidden" }}>
                      <span style={{ display: "flex", alignItems: "center", color: "#111827" }}>
                        {isGroup ? (
                          isExpanded ? (
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M6 9l6 6 6-6"/></svg>
                          ) : (
                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M9 18l6-6-6-6"/></svg>
                          )
                        ) : (
                          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/></svg>
                        )}
                      </span>
                      <span style={{ fontSize: "14px", fontWeight: "600", color: "#111827", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                        {task.name}
                      </span>
                    </div>
                    
                    <div style={{ fontSize: "13px", fontWeight: "500", color: isPaused || isQueued ? "#ef4444" : "#4b5563" }}>
                      {isGroup ? `${task.finishedItems} / ${task.totalItems} done` : (isPaused ? "PAUSED" : isQueued ? "WAITING" : p.speed)}
                    </div>
                    <div style={{ fontSize: "13px", fontWeight: "500", color: "#4b5563" }}>
                      {isPaused || isQueued || isGroup ? "--" : p.eta}
                    </div>
                    <div style={{ fontSize: "13px", color: isPaused || isQueued ? "#9ca3af" : "#111827", fontWeight: "700", textAlign: "right" }}>
                      {p.percent}%
                    </div>

                    <div style={{ display: "flex", gap: "8px", justifyContent: "flex-end" }}>
                      {isPaused || isQueued ? (
                        <button style={btnStyle} onClick={(e) => { e.stopPropagation(); handleControl(task, 0); }} title="Resume">
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><polygon points="5 3 19 12 5 21 5 3"/></svg>
                        </button>
                      ) : (
                        <button style={btnStyle} onClick={(e) => { e.stopPropagation(); handleControl(task, 1); }} title="Pause">
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>
                        </button>
                      )}
                      <button style={{...btnStyle, color: "#ef4444", borderColor: "#ef4444"}} onClick={(e) => { e.stopPropagation(); handleControl(task, 2); }} title="Cancel">
                        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                      </button>
                    </div>
                  </div>
                  
                  {!isGroup && (
                    <div style={{ padding: "0 20px 16px 20px", backgroundColor: isExpanded ? "#f3f4f6" : "transparent" }}>
                      <div style={{ width: "100%", height: "8px", backgroundColor: "#f3f4f6", border: "1px solid #111827", borderRadius: "0", overflow: "hidden" }}>
                        <div style={{ width: `${p.percent}%`, height: "100%", backgroundColor: isPaused || isQueued ? "#9ca3af" : "#111827", transition: "width 0.3s ease-out" }} />
                      </div>
                    </div>
                  )}

                  {/* Render Children if Group is Expanded */}
                  {isGroup && isExpanded && children.map(child => {
                    const cp = progressMap[child.id] || { percent: 0, speed: "Starting...", eta: "--" };
                    const cPaused = child.status === "Paused";
                    const cQueued = child.status === "Queued";
                    return (
                      <div key={child.id} style={{ display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 80px 60px 100px", alignItems: "center", padding: "10px 20px 10px 50px", backgroundColor: "#f9fafb", borderTop: "1px solid #e5e7eb" }}>
                        <div style={{ display: "flex", alignItems: "center", gap: "8px", fontSize: "12px", fontWeight: "600", color: "#4b5563", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><polyline points="13 2 13 9 20 9"/></svg>
                          {child.name}
                        </div>
                        <div style={{ fontSize: "11px", fontWeight: "600", color: cPaused || cQueued ? "#ef4444" : "#6b7280" }}>{cPaused ? "PAUSED" : cQueued ? "WAIT" : cp.speed}</div>
                        <div style={{ fontSize: "11px", fontWeight: "600", color: "#6b7280" }}>{cPaused || cQueued ? "--" : cp.eta}</div>
                        <div style={{ fontSize: "11px", fontWeight: "700", color: cPaused || cQueued ? "#9ca3af" : "#111827", textAlign: "right" }}>{cp.percent}%</div>
                        <div style={{ display: "flex", gap: "4px", justifyContent: "flex-end" }}>
                          <button style={{...btnStyle, padding: "4px", color: "#ef4444", borderColor: "#ef4444"}} onClick={() => handleControl(child, 2)}>
                            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                          </button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

// --------------------------------------------------------
// Styling (Matches Neo-Brutalist exactly)
// --------------------------------------------------------
const styles: { [key: string]: React.CSSProperties } = {
  header: { display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "20px" },
  listContainer: { backgroundColor: "#ffffff", borderRadius: "0", border: "1px solid #111827", display: "flex", flexDirection: "column", flex: 1, overflow: "hidden", position: "relative", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)" },
  listHeaderRow: { display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 80px 60px 100px", padding: "12px 20px", backgroundColor: "#f3f4f6", borderBottom: "1px solid #111827", fontWeight: "700", color: "#111827", fontSize: "11px", alignItems: "center", textTransform: "uppercase", letterSpacing: "1px" },
  statusState: { textAlign: "center", padding: "48px", color: "#111827", fontSize: "12px", fontWeight: "600", textTransform: "uppercase", letterSpacing: "1px" },
};

const btnStyle: React.CSSProperties = { 
  display: "flex", 
  alignItems: "center", 
  justifyContent: "center", 
  background: "transparent", 
  border: "1px solid #d1d5db", 
  cursor: "pointer", 
  padding: "6px", 
  borderRadius: "0", 
  color: "#111827", 
  transition: "background 0.2s, border-color 0.2s" 
};