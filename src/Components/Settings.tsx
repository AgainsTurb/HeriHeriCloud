import { useState, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";

interface SettingsState {
  concurrentUploads: number;
  concurrentDownloads: number;
  downloadPath: string;
  useDefaultDownloadPath: boolean;
  uploadSpeedLimit: number;
  downloadSpeedLimit: number;
  unlimitedUpload: boolean;
  unlimitedDownload: boolean;
  notifyUpload: boolean;
  notifyDownload: boolean;
  notifySound: boolean;
  enableWebDAV: boolean;
  webdavPort: number;
  webdavUser: string;
  webdavPass: string;
}

export default function Settings({ isMobile, AppLogo, AppAuth }: any) {
  const { t, i18n } = useTranslation();
  const [activeTab, setActiveTab] = useState<"General" | "Transfer" | "Notification">("General");
  const [alertData, setAlertData] = useState<{ title: string, msg: string } | null>(null);

  const [localIp, setLocalIp] = useState("192.168.x.x");

  useEffect(() => {
    invoke<string>("get_local_ip")
      .then(ip => setLocalIp(ip))
      .catch(console.error);
  }, []);

  const [settings, setSettings] = useState<SettingsState>(() => {
    const defaultSettings: SettingsState = {
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
      enableWebDAV: true,
      webdavPort: 8888,
      webdavUser: "admin",
      webdavPass: "admin",
    };

    const saved = localStorage.getItem("heriheri_config");
    return saved ? { ...defaultSettings, ...JSON.parse(saved) } : defaultSettings;
  });

  const handleSave = async () => {
    localStorage.setItem("heriheri_config", JSON.stringify(settings));
    
    const upLimit = settings.unlimitedUpload ? 0 : settings.uploadSpeedLimit;
    const downLimit = settings.unlimitedDownload ? 0 : settings.downloadSpeedLimit;
    await invoke("vfs_update_speed_limits", { uploadLimit: upLimit, downloadLimit: downLimit }).catch(console.error);

    // Push WebDAV settings to Rust
    await invoke("set_webdav_config", { 
      port: Number(settings.webdavPort), 
      username: settings.webdavUser, 
      password: settings.webdavPass 
    }).catch(console.error);

    setAlertData({
      title: t("Configuration Saved!"),
      msg: t("Your changes have been saved. Please restart the app if you updated the WebDAV Port to apply network socket modifications.")
    });
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", paddingTop: "calc(env(safe-area-inset-top, 0px) + 0px)" }}>
      <header style={{ ...styles.header, marginBottom: isMobile ? "12px" : "20px" }}>
        <div style={{ display: "flex", gap: isMobile ? "12px" : "24px", alignItems: "center" }}>
          <h2 
            style={{...styles.tabTitle, fontSize: isMobile ? "18px" : "18px", color: activeTab === "General" ? "#111827" : "#9ca3af", borderBottom: activeTab === "General" ? "2px solid #111827" : "2px solid transparent"}} 
            onClick={() => setActiveTab("General")}
          >
            {t("General")}
          </h2>
          <h2 
            style={{...styles.tabTitle, fontSize: isMobile ? "18px" : "18px", color: activeTab === "Transfer" ? "#111827" : "#9ca3af", borderBottom: activeTab === "Transfer" ? "2px solid #111827" : "2px solid transparent"}} 
            onClick={() => setActiveTab("Transfer")}
          >
            {t("Transfer")}
          </h2>
          <h2 
            style={{...styles.tabTitle, fontSize: isMobile ? "18px" : "18px", color: activeTab === "Notification" ? "#111827" : "#9ca3af", borderBottom: activeTab === "Notification" ? "2px solid #111827" : "2px solid transparent"}} 
            onClick={() => setActiveTab("Notification")}
          >
            {t("Notification")}
          </h2>
        </div>
      </header>

      <div style={styles.container}>
        <div style={{ ...styles.scrollArea, padding: isMobile ? "16px" : "32px", marginBottom: isMobile ? "60px" : "0px" }}>
          {activeTab === "General" && (
            <div style={{ ...styles.section, gap: isMobile ? "14px" : "24px" }}>

              {isMobile && (
                <>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                    {AppLogo}
                    {AppAuth}
                  </div>
                  <hr style={styles.divider} />
                </>
              )}

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

              <hr style={styles.divider} />

              <h3 style={styles.sectionTitle}>{t("WebDAV Local Mount")}</h3>
              
              {/* --- MINIMUM FIX: WebDAV URL Display --- */}
              <div style={{ marginBottom: "16px", padding: "12px", backgroundColor: "#f9fafb", border: "1px dashed #d1d5db", fontSize: "12px", color: "#4b5563", lineHeight: "1.6" }}>
                <div style={{ marginBottom: "4px" }}>
                  <strong>{t("Local Access:")}</strong> <span style={{ fontFamily: "monospace", color: "#111827", userSelect: "all" }}>http://127.0.0.1:{settings.webdavPort}/dav</span>
                </div>
                <div>
                  <strong>{t("Network Access:")}</strong> <span style={{ fontFamily: "monospace", color: "#111827", userSelect: "all" }}>http://{localIp}:{settings.webdavPort}/dav</span>
                </div>
              </div>

              <div style={{ ...styles.grid2Col, gridTemplateColumns: isMobile ? "1fr" : "1fr 1fr", gap: isMobile ? "12px" : "24px" }}>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Mount Port")}</label>
                  <input 
                    style={styles.input} 
                    type="number" 
                    value={settings.webdavPort} 
                    onChange={(e) => setSettings({...settings, webdavPort: parseInt(e.target.value) || 8888})} 
                  />
                </div>
              </div>
              <div style={{ ...styles.grid2Col, gridTemplateColumns: isMobile ? "1fr" : "1fr 1fr", gap: isMobile ? "12px" : "24px" }}>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Username")}</label>
                  <input 
                    style={styles.input} 
                    type="text" 
                    value={settings.webdavUser} 
                    onChange={(e) => setSettings({...settings, webdavUser: e.target.value})} 
                  />
                </div>
                <div style={styles.inputGroup}>
                  <label style={styles.inputLabel}>{t("Password")}</label>
                  <input 
                    style={styles.input} 
                    type="text" 
                    value={settings.webdavPass} 
                    onChange={(e) => setSettings({...settings, webdavPass: e.target.value})} 
                  />
                </div>
              </div>
            </div>
          )}

          {activeTab === "Transfer" && (
            <div style={{ ...styles.section, gap: isMobile ? "14px" : "24px" }}>
              <h3 style={styles.sectionTitle}>{t("Concurrent Tasks")}</h3>
              <div style={{ ...styles.grid2Col, gridTemplateColumns: isMobile ? "1fr" : "1fr 1fr", gap: isMobile ? "12px" : "24px" }}>
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
                
                <div style={{ display: "flex", flexDirection: isMobile ? "column" : "row", gap: isMobile ? "12px" : "8px", alignItems: isMobile ? "flex-start" : "center" }}>
                  
                  {/* Keep Input and Browse button together on the same line */}
                  <div style={{ display: "flex", gap: "8px", width: isMobile ? "100%" : "auto", flex: 1 }}>
                    <input style={{...styles.readOnlyInput, flex: 1, minWidth: 0}} value={settings.downloadPath} readOnly />
                    <button 
                      style={styles.secondaryButton} 
                      onClick={async () => {
                        const selected = await open({ directory: true });
                        if (selected && !Array.isArray(selected)) setSettings({...settings, downloadPath: selected});
                      }}
                    >
                      {t("Browse")}
                    </button>
                  </div>

                  {/* Drop to next line and scale down on Mobile */}
                  <div 
                    style={{...styles.checkboxWrapper, marginLeft: isMobile ? "0px" : "8px"}} 
                    onClick={() => setSettings({...settings, useDefaultDownloadPath: !settings.useDefaultDownloadPath})}
                  >
                    <div style={{...styles.checkbox, width: isMobile ? "14px" : "18px", height: isMobile ? "14px" : "18px", backgroundColor: settings.useDefaultDownloadPath ? "#111827" : "#ffffff"}}>
                      {settings.useDefaultDownloadPath && <svg width={isMobile ? "8" : "10"} height={isMobile ? "8" : "10"} viewBox="0 0 24 24" fill="none" stroke="#ffffff" strokeWidth="3" strokeLinecap="square" strokeLinejoin="miter"><polyline points="20 6 9 17 4 12"/></svg>}
                    </div>
                    <span style={{...styles.checkboxLabel, fontSize: isMobile ? "11px" : "13px"}}>{t("Default")}</span>
                  </div>

                </div>
              </div>

              <hr style={styles.divider} />

              <h3 style={styles.sectionTitle}>{t("Speed Limits")}</h3>
              <div style={{ ...styles.grid2Col, gridTemplateColumns: isMobile ? "1fr" : "1fr 1fr", gap: isMobile ? "12px" : "24px" }}>
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
            <div style={{ ...styles.section, gap: isMobile ? "14px" : "24px" }}>
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
        </div>

        {/* Fixed Footer for Save Action */}
        <div style={{ ...styles.footer, padding: isMobile ? "12px 16px 45px 16px" : "16px 32px" }}>
          <button style={{ ...styles.primaryButton, width: isMobile ? "100%" : "auto", justifyContent: "center" }} onClick={handleSave}>
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
  section: { display: "flex", flexDirection: "column", gap: "16px" },
  sectionTitle: { margin: "0 0 8px 0", fontSize: "14px", fontWeight: "800", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  grid2Col: { display: "grid", gridTemplateColumns: "1fr 1fr", gap: "24px" },
  inputGroup: { display: "flex", flexDirection: "column", gap: "4px" },
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
  modalOverlay: { position: "fixed", top: 0, left: 0, right: 0, bottom: 0, backgroundColor: "rgba(255, 255, 255, 0.9)", display: "flex", justifyContent: "center", alignItems: "center", zIndex: 999 },
  modalBox: { backgroundColor: "#ffffff", padding: "32px", borderRadius: "0", border: "2px solid #111827", width: "360px", boxShadow: "8px 8px 0px 0px rgba(17, 24, 39, 1)", display: "flex", flexDirection: "column", gap: "24px" },
  modalTitle: { margin: 0, fontSize: "16px", fontWeight: "800", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  modalText: { margin: 0, fontSize: "13px", color: "#4b5563", lineHeight: "1.5" },
  modalActions: { display: "flex", justifyContent: "flex-end", gap: "12px", marginTop: "8px" },
};