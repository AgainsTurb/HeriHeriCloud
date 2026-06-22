import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";

export default function Finished() {
  const { t } = useTranslation();
  const [finishedTasks, setFinishedTasks] = useState<any[]>([]);
  const [subTab, setSubTab] = useState<"Upload" | "Download">("Upload");

  const loadTasks = () => {
    const finished = JSON.parse(localStorage.getItem("heriheri_finished") || "[]");
    finished.sort((a: any, b: any) => b.time - a.time);
    setFinishedTasks(finished);
  };

  useEffect(() => {
    loadTasks();
    window.addEventListener("TASK_END", loadTasks);
    return () => window.removeEventListener("TASK_END", loadTasks);
  }, []);

  function clearHistory() {
    if (confirm(`Are you sure you want to clear the ${subTab} history?`)) {
      // Only clear the tasks for the currently active tab
      const newHistory = finishedTasks.filter(t => t.type !== subTab);
      localStorage.setItem("heriheri_finished", JSON.stringify(newHistory));
      loadTasks();
    }
  }

  function formatTime(ms: number) {
    const d = new Date(ms);
    return `${d.getHours()}:${String(d.getMinutes()).padStart(2,'0')} ${String(d.getSeconds()).padStart(2,'0')}s`;
  }

  const displayedTasks = finishedTasks.filter(t => t.type === subTab);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      <header style={styles.header}>
        <div style={{ display: "flex", gap: "24px", alignItems: "center" }}>
          <h2 style={{...styles.tabTitle, color: subTab === "Upload" ? "#111827" : "#9ca3af", borderBottom: subTab === "Upload" ? "2px solid #111827" : "2px solid transparent"}} onClick={() => setSubTab("Upload")}>{t("Uploaded")}</h2>
          <h2 style={{...styles.tabTitle, color: subTab === "Download" ? "#111827" : "#9ca3af", borderBottom: subTab === "Download" ? "2px solid #111827" : "2px solid transparent"}} onClick={() => setSubTab("Download")}>{t("Downloaded")}</h2>
        </div>
        <button style={styles.secondaryButton} onClick={clearHistory}>{t("Clear History")}</button>
      </header>

      <div style={styles.listContainer}>
        <div style={styles.listHeaderRow}>
          <div style={styles.cellName}>{t("File Name")}</div>
          <div style={styles.cellDefault}>{t("Operation")}</div>
          <div style={styles.cellDefault}>{t("Status")}</div>
          <div style={styles.cellDefault}>{t("Time")}</div>
        </div>

        {displayedTasks.length === 0 ? (
          <div style={styles.statusState}>{t("No history found.")}</div>
        ) : (
          <div style={styles.listBody}>
            {displayedTasks.map((task) => (
              <div key={task.id} style={styles.listRow}>
                <div style={styles.cellName}>
                  <span style={styles.icon}>
                    {task.type === "Upload" ? (
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12"/></svg>
                    ) : (
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/></svg>
                    )}
                  </span>
                  <span style={styles.itemName} title={task.error}>{task.name}</span>
                </div>
                <div style={styles.cellDefault}>{task.type}</div>
                <div style={{
                  ...styles.cellDefault, 
                  color: task.status === "Success" ? "#10b981" : task.status === "Skipped" ? "#f59e0b" : "#ef4444", 
                  fontWeight: "700"
                }}>
                  {task.status}
                </div>
                <div style={styles.cellDefault}>{formatTime(task.time)}</div>
              </div>
            ))}
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
  header: { display: "flex", justifyContent: "space-between", alignItems: "flex-end", marginBottom: "20px", borderBottom: "2px solid #111827" },
  tabTitle: { margin: 0, paddingBottom: "12px", cursor: "pointer", fontSize: "18px", fontWeight: "800", transition: "color 0.2s", textTransform: "uppercase", letterSpacing: "1px" },
  listContainer: { backgroundColor: "#ffffff", borderRadius: "0", border: "1px solid #111827", display: "flex", flexDirection: "column", flex: 1, overflow: "hidden", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)" },
  listHeaderRow: { display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 100px 100px", padding: "12px 20px", backgroundColor: "#f3f4f6", borderBottom: "1px solid #111827", fontWeight: "700", color: "#111827", fontSize: "11px", alignItems: "center", textTransform: "uppercase", letterSpacing: "1px" },
  listBody: { overflowY: "auto", flex: 1 },
  listRow: { display: "grid", gridTemplateColumns: "minmax(200px, 3fr) 100px 100px 100px", padding: "10px 20px", borderBottom: "1px solid #e5e7eb", alignItems: "center" },
  cellName: { display: "flex", alignItems: "center", gap: "12px", overflow: "hidden" },
  cellDefault: { fontSize: "13px", color: "#4b5563", whiteSpace: "nowrap" },
  icon: { display: "flex", alignItems: "center", color: "#111827" },
  itemName: { fontSize: "13px", fontWeight: "600", color: "#111827", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
  statusState: { textAlign: "center", padding: "48px", color: "#111827", fontSize: "12px", fontWeight: "600", textTransform: "uppercase", letterSpacing: "1px" },
  secondaryButton: { backgroundColor: "#ffffff", color: "#111827", padding: "8px 16px", borderRadius: "0", border: "1px solid #111827", fontSize: "12px", fontWeight: "700", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px", marginBottom: "12px", transition: "background 0.2s" }
};