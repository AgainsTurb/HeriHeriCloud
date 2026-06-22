import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

export default function Settings() {
  const { t, i18n } = useTranslation();
  const [activeTab, setActiveTab] = useState<"General" | "Transfer" | "Notification">("General");

  const [settings, setSettings] = useState(() => {
    const saved = localStorage.getItem("heriheri_config");
    return saved ? JSON.parse(saved) : {
      concurrentUploads: 2,
      concurrentDownloads: 2,
      downloadPath: "",
      useDefaultDownloadPath: false,
      uploadSpeedLimit: 0,
      downloadSpeedLimit: 0,
      unlimitedUpload: true,
      unlimitedDownload: true,
      notifyUpload: true,
      notifyDownload: true,
      notifySound: false,
    };
  });

  const handleSave = async () => {
    localStorage.setItem("heriheri_config", JSON.stringify(settings));
    
    const upLimit = settings.unlimitedUpload ? 0 : settings.uploadSpeedLimit;
    const downLimit = settings.unlimitedDownload ? 0 : settings.downloadSpeedLimit;
    await invoke("vfs_update_speed_limits", { uploadLimit: upLimit, downloadLimit: downLimit }).catch(console.error);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      <header style={styles.header}>
        <div style={{ display: "flex", gap: "24px", alignItems: "center" }}>
          <h2 
            style={{...styles.tabTitle, color: activeTab === "General" ? "#111827" : "#9ca3af", borderBottom: activeTab === "General" ? "2px solid #111827" : "2px solid transparent"}} 
            onClick={() => setActiveTab("General")}
          >
            {t("General")}
          </h2>
          <h2 
            style={{...styles.tabTitle, color: activeTab === "Transfer" ? "#111827" : "#9ca3af", borderBottom: activeTab === "Transfer" ? "2px solid #111827" : "2px solid transparent"}} 
            onClick={() => setActiveTab("Transfer")}
          >
            {t("Transfer")}
          </h2>
          <h2 
            style={{...styles.tabTitle, color: activeTab === "Notification" ? "#111827" : "#9ca3af", borderBottom: activeTab === "Notification" ? "2px solid #111827" : "2px solid transparent"}} 
            onClick={() => setActiveTab("Notification")}
          >
            {t("Notification")}
          </h2>
        </div>
      </header>

      <div style={styles.container}>
        <div style={styles.scrollArea}>
          {activeTab === "General" && (
            <div style={styles.section}>
              <h3 style={styles.sectionTitle}>{t("Language")}</h3>
              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Display Language")}</label>
                <select 
                  style={{...styles.input, cursor: "pointer", width: "200px"}} 
                  value={i18n.language?.startsWith('zh') ? 'zh' : 'en'} 
                  onChange={(e) => i18n.changeLanguage(e.target.value)}
                >
                  <option value="en">English</option>
                  <option value="zh">中文 (简体)</option>
                </select>
              </div>
            </div>
          )}
          
          {activeTab === "Transfer" && (
            <div style={styles.section}>
              <h3 style={styles.sectionTitle}>{t("Concurrent Tasks")}</h3>
              <div style={styles.grid2Col}>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Max Concurrent Uploads")}</label>
                  <input 
                    style={styles.input} 
                    type="number" 
                    min="1" max="10" 
                    value={settings.concurrentUploads}
                    onChange={(e) => setSettings({...settings, concurrentUploads: parseInt(e.target.value) || 1})}
                  />
                </div>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Max Concurrent Downloads")}</label>
                  <input 
                    style={styles.input} 
                    type="number" 
                    min="1" max="10" 
                    value={settings.concurrentDownloads}
                    onChange={(e) => setSettings({...settings, concurrentDownloads: parseInt(e.target.value) || 1})}
                  />
                </div>
              </div>

              <hr style={styles.divider} />

              <h3 style={styles.sectionTitle}>{t("Download Location")}</h3>
              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Target Download Path")}</label>
                <div style={{ display: "flex", gap: "8px" }}>
                  <input style={{...styles.readOnlyInput, flex: 1}} value={settings.downloadPath} readOnly />
                  <button 
                    style={styles.secondaryButton} 
                    onClick={async () => {
                      const selected = await open({ directory: true });
                      if (selected && !Array.isArray(selected)) setSettings({...settings, downloadPath: selected});
                    }}
                  >
                    {t("Browse")}
                  </button>
                  <div 
                    style={{...styles.checkboxWrapper, marginLeft: "8px"}} 
                    onClick={() => setSettings({...settings, useDefaultDownloadPath: !settings.useDefaultDownloadPath})}
                  >
                    <div style={{...styles.checkbox, backgroundColor: settings.useDefaultDownloadPath ? "#111827" : "#ffffff"}}>
                      {settings.useDefaultDownloadPath && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="3" strokeLinecap="square" strokeLinejoin="miter"><polyline points="20 6 9 17 4 12"/></svg>}
                    </div>
                    <span style={styles.checkboxLabel}>{t("Default")}</span>
                  </div>
                </div>
              </div>

              <hr style={styles.divider} />

              <h3 style={styles.sectionTitle}>{t("Speed Limits")}</h3>
              <div style={styles.grid2Col}>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Upload Speed Limit (KB/s)")}</label>
                  <div style={{ display: "flex", alignItems: "center", gap: "12px", marginTop: "4px" }}>
                    <div 
                      style={styles.checkboxWrapper} 
                      onClick={() => setSettings({...settings, unlimitedUpload: !settings.unlimitedUpload})}
                    >
                      <div style={{...styles.checkbox, backgroundColor: settings.unlimitedUpload ? "#111827" : "#ffffff"}}>
                        {settings.unlimitedUpload && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="3" strokeLinecap="square" strokeLinejoin="miter"><polyline points="20 6 9 17 4 12"/></svg>}
                      </div>
                      <span style={styles.checkboxLabel}>{t("Unlimited")}</span>
                    </div>
                    <input 
                      style={{...styles.input, flex: 1, opacity: settings.unlimitedUpload ? 0.5 : 1}} 
                      type="number" 
                      min="0"
                      disabled={settings.unlimitedUpload}
                      value={settings.uploadSpeedLimit}
                      onChange={(e) => setSettings({...settings, uploadSpeedLimit: parseInt(e.target.value) || 0})}
                    />
                  </div>
                </div>

                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Download Speed Limit (KB/s)")}</label>
                  <div style={{ display: "flex", alignItems: "center", gap: "12px", marginTop: "4px" }}>
                    <div 
                      style={styles.checkboxWrapper} 
                      onClick={() => setSettings({...settings, unlimitedDownload: !settings.unlimitedDownload})}
                    >
                      <div style={{...styles.checkbox, backgroundColor: settings.unlimitedDownload ? "#111827" : "#ffffff"}}>
                        {settings.unlimitedDownload && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="3" strokeLinecap="square" strokeLinejoin="miter"><polyline points="20 6 9 17 4 12"/></svg>}
                      </div>
                      <span style={styles.checkboxLabel}>{t("Unlimited")}</span>
                    </div>
                    <input 
                      style={{...styles.input, flex: 1, opacity: settings.unlimitedDownload ? 0.5 : 1}} 
                      type="number" 
                      min="0"
                      disabled={settings.unlimitedDownload}
                      value={settings.downloadSpeedLimit}
                      onChange={(e) => setSettings({...settings, downloadSpeedLimit: parseInt(e.target.value) || 0})}
                    />
                  </div>
                </div>
              </div>
            </div>
          )}

          {activeTab === "Notification" && (
            <div style={styles.section}>
              <h3 style={styles.sectionTitle}>{t("System Notifications")}</h3>
              
              <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
                <div 
                  style={styles.checkboxWrapper} 
                  onClick={() => setSettings({...settings, notifyUpload: !settings.notifyUpload})}
                >
                  <div style={{...styles.checkbox, backgroundColor: settings.notifyUpload ? "#111827" : "#ffffff"}}>
                    {settings.notifyUpload && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="3" strokeLinecap="square" strokeLinejoin="miter"><polyline points="20 6 9 17 4 12"/></svg>}
                  </div>
                  <span style={styles.checkboxLabel}>{t("Notify when an Upload task finishes")}</span>
                </div>

                <div 
                  style={styles.checkboxWrapper} 
                  onClick={() => setSettings({...settings, notifyDownload: !settings.notifyDownload})}
                >
                  <div style={{...styles.checkbox, backgroundColor: settings.notifyDownload ? "#111827" : "#ffffff"}}>
                    {settings.notifyDownload && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="3" strokeLinecap="square" strokeLinejoin="miter"><polyline points="20 6 9 17 4 12"/></svg>}
                  </div>
                  <span style={styles.checkboxLabel}>{t("Notify when a Download task finishes")}</span>
                </div>

                <hr style={styles.divider} />

                <h3 style={styles.sectionTitle}>{t("Audio")}</h3>
                <div 
                  style={styles.checkboxWrapper} 
                  onClick={() => setSettings({...settings, notifySound: !settings.notifySound})}
                >
                  <div style={{...styles.checkbox, backgroundColor: settings.notifySound ? "#111827" : "#ffffff"}}>
                    {settings.notifySound && <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="3" strokeLinecap="square" strokeLinejoin="miter"><polyline points="20 6 9 17 4 12"/></svg>}
                  </div>
                  <span style={styles.checkboxLabel}>{t("Play a sound on task completion")}</span>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Fixed Footer for Save Action */}
        <div style={styles.footer}>
          <button style={styles.primaryButton} onClick={handleSave}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter" style={{ marginRight: "8px" }}><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><polyline points="17 21 17 13 7 13 7 21"/><polyline points="7 3 7 8 15 8"/></svg>
            {t("Save Configuration")}
          </button>
        </div>
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
  container: { backgroundColor: "#ffffff", borderRadius: "0", border: "1px solid #111827", display: "flex", flexDirection: "column", flex: 1, overflow: "hidden", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)" },
  scrollArea: { padding: "32px", overflowY: "auto", flex: 1 },
  section: { display: "flex", flexDirection: "column", gap: "24px" },
  sectionTitle: { margin: "0 0 8px 0", fontSize: "14px", fontWeight: "800", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  grid2Col: { display: "grid", gridTemplateColumns: "1fr 1fr", gap: "24px" },
  inputGroup: { display: "flex", flexDirection: "column", gap: "8px" },
  inputLabel: { fontSize: "11px", fontWeight: "700", color: "#4b5563", textTransform: "uppercase", letterSpacing: "1px" },
  input: { padding: "10px 12px", backgroundColor: "#ffffff", color: "#111827", border: "1px solid #111827", borderRadius: "0", fontSize: "13px", outline: "none", fontWeight: "600", width: "100%", boxSizing: "border-box" },
  readOnlyInput: { padding: "10px 12px", backgroundColor: "#f3f4f6", border: "1px solid #111827", borderRadius: "0", fontSize: "13px", color: "#4b5563", outline: "none", fontWeight: "600", cursor: "default" },
  divider: { border: "none", borderTop: "1px dashed #d1d5db", margin: "8px 0", width: "100%" },
  checkboxWrapper: { display: "flex", alignItems: "center", gap: "12px", cursor: "pointer", userSelect: "none" },
  checkbox: { width: "18px", height: "18px", border: "2px solid #111827", display: "flex", justifyContent: "center", alignItems: "center", transition: "background-color 0.1s" },
  checkboxLabel: { fontSize: "13px", fontWeight: "700", color: "#111827", textTransform: "uppercase", letterSpacing: "0.5px" },
  footer: { backgroundColor: "#f9fafb", padding: "16px 32px", borderTop: "1px solid #111827", display: "flex", justifyContent: "flex-end" },
  primaryButton: { display: "flex", alignItems: "center", backgroundColor: "#111827", color: "#ffffff", padding: "10px 24px", borderRadius: "0", border: "1px solid #111827", fontSize: "12px", fontWeight: "700", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px", transition: "background 0.2s" },
  secondaryButton: { backgroundColor: "#ffffff", color: "#111827", padding: "10px 16px", borderRadius: "0", border: "1px solid #111827", fontSize: "12px", fontWeight: "700", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px", transition: "background 0.2s" },
};