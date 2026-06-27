import { getVersion } from "@tauri-apps/api/app";
import { type } from "@tauri-apps/plugin-os";
import { open as openBrowser } from "@tauri-apps/plugin-shell";
import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { keepScreenOn } from "tauri-plugin-keep-screen-on-api";
import "./App.css";

import Home from "./Components/Home";
import Transfer from "./Components/Transfer";
import Settings from "./Components/Settings";
import Rent from "./Components/Rent";

export default function App() {
  const { t } = useTranslation();
  const [status, setStatus] = useState("Disconnected");
  const [activeTab, setActiveTab] = useState("home");
  
  const [username, setUsername] = useState("");
  
  const [showLogin, setShowLogin] = useState(false);
  const [showRegister, setShowRegister] = useState(false);
  const [loginForm, setLoginForm] = useState({ phone: "", password: "" });

  const [regForm, setRegForm] = useState({ phone: "", code: "", password: "", confirm: "" });
  const [countdown, setCountdown] = useState(0);

  const [appVersion, setAppVersion] = useState("");
  const [updateAvailable, setUpdateAvailable] = useState<{ version: string, body: string, url: string } | null>(null);
  const [showUpdateModal, setShowUpdateModal] = useState(false);

  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    try {
      const os = type();
      setIsMobile(os === 'android' || os === 'ios');
    } catch (err) {
      console.warn("Failed to detect OS type:", err);
    }
  }, []);

  const isUploadingBatch = useRef(false);
  const isSyncing = useRef(false);

  useEffect(() => {
    const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
    const upLimit = config.unlimitedUpload !== false ? 0 : (config.uploadSpeedLimit || 0);
    const downLimit = config.unlimitedDownload !== false ? 0 : (config.downloadSpeedLimit || 0);
    invoke("vfs_update_speed_limits", { uploadLimit: upLimit, downloadLimit: downLimit }).catch(console.error);
  }, []);

  useEffect(() => {
    // 1. Read from disk or fallback to the system defaults immediately
    const saved = localStorage.getItem("heriheri_config");
    const config = saved ? JSON.parse(saved) : {
      enableWebDAV: true,
      webdavPort: 8888,
      webdavUser: "admin",
      webdavPass: "admin",
    };
    
    // 2. If it's a first-time boot, commit these defaults to disk so the file exists
    if (!saved) {
      localStorage.setItem("heriheri_config", JSON.stringify(config));
    }
    
    // 3. Fire the ignition command to Rust
    if (config.enableWebDAV) {
      invoke("boot_webdav_server", {
        port: Number(config.webdavPort) || 8888,
        username: config.webdavUser || "admin",
        password: config.webdavPass || "admin"
      }).catch(err => console.error("Failed to boot WebDAV:", err));
    }
  }, []);

  useEffect(() => {
    if (countdown > 0) {
      const timer = setTimeout(() => setCountdown(countdown - 1), 1000);
      return () => clearTimeout(timer);
    }
  }, [countdown]);

  useEffect(() => {
    const savedYlogin = localStorage.getItem("ylogin");
    const savedPhpdisk = localStorage.getItem("phpdisk_info");
    const savedPhone = localStorage.getItem("phone");

    if (savedYlogin && savedPhpdisk && savedPhone) {
      restoreSession(savedYlogin, savedPhpdisk, savedPhone);
    }
  }, []);

  useEffect(() => {
    async function initVersion() {
      try {
        const ver = await getVersion();
        setAppVersion(ver);
        
        const res = await fetch("https://api.github.com/repos/AgainsTurb/HeriHeriCloud/releases/latest");
        if (!res.ok) return;
        const data = await res.json();
        
        // Strip the 'v' from the GitHub tag (e.g., 'v1.0.0' -> '1.0.0')
        const latestVer = data.tag_name.replace(/^v/, '');
        
        if (latestVer !== ver) {
          const osType = await type(); // Returns 'windows', 'macos', 'linux', etc.
          let targetUrl = data.html_url; // Fallback to the general release page
          
          // Hunt for the exact OS artifact
          const asset = data.assets.find((a: any) => {
            const name = a.name.toLowerCase();
            if (osType === 'windows' && (name.endsWith('.exe') || name.endsWith('.msi'))) return true;
            if (osType === 'macos' && (name.endsWith('.dmg') || name.endsWith('.app.tar.gz'))) return true;
            if (osType === 'linux' && (name.endsWith('.appimage') || name.endsWith('.deb') || name.endsWith('.rpm'))) return true;
            if (osType === 'android' && name.endsWith('.apk')) return true;
            return false;
          });

          // Prepend the proxy for mainland China acceleration
          if (asset) {
            targetUrl = `https://gh-proxy.org/${asset.browser_download_url}`;
          }

          setUpdateAvailable({
            version: data.tag_name,
            body: data.body,
            url: targetUrl
          });
        }
      } catch (err) {
        console.error("Version check failed:", err);
      }
    }
    initVersion();
  }, []);

  async function triggerSystemNotification(title: string, body: string, playSound: boolean) {
    if (playSound) {
      new Audio('/chime.mp3').play().catch(e => console.warn("Audio play failed:", e));
    }
    
    let permissionGranted = await isPermissionGranted();
    if (!permissionGranted) {
      const permission = await requestPermission();
      permissionGranted = permission === 'granted';
    }
    if (permissionGranted) {
      sendNotification({ title, body });
    }
  }

  async function restoreSession(yl: string, pd: string, phone: string) {
    setStatus("Connecting...");
    try {
      const success = await invoke<boolean>("set_lanzou_cookies", { ylogin: yl, phpdiskInfo: pd, phone });
      if (success) {
        setStatus("Connected");
        setUsername(phone.replace(/(\d{3})\d{4}(\d{4})/, "$1****$2"));
      } else {
        handleLogout();
      }
    } catch {
      handleLogout();
    }
  }

  async function handleLoginSubmit(e: React.FormEvent) {
    e.preventDefault();
    setStatus("Connecting...");
    try {
      const [yl, pd] = await invoke<[string, string]>("login", { 
        username: loginForm.phone, 
        password: loginForm.password 
      });
      
      localStorage.setItem("ylogin", yl);
      localStorage.setItem("phpdisk_info", pd);
      localStorage.setItem("phone", loginForm.phone);
      
      setStatus("Connected");
      setUsername(loginForm.phone.replace(/(\d{3})\d{4}(\d{4})/, "$1****$2"));
      setShowLogin(false);
    } catch (error) {
      alert(`Login Error: ${error}`);
      setStatus("Disconnected");
    }
  }

  async function handleRequestSMS() {
    if (!regForm.phone || regForm.phone.length < 11) {
      alert("Please enter a valid phone number.");
      return;
    }
    
    try {
      await invoke("request_register_sms", { phone: regForm.phone });
      setCountdown(60);
      alert("SMS Sent! Please check your phone.");
    } catch (error) {
      alert(`Error sending SMS: ${error}`);
    }
  }

  async function handleRegisterSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (regForm.password !== regForm.confirm) {
      alert("Passwords do not match.");
      return;
    }
    if (!regForm.code) {
      alert("Please enter the verification code.");
      return;
    }

    try {
      setStatus("Registering...");
      await invoke("submit_register", { 
        phone: regForm.phone, 
        code: regForm.code, 
        password: regForm.password 
      });
      
      alert("Registration successful! You can now log in.");
      setShowRegister(false);
      setShowLogin(true); 
      setStatus("Disconnected");
    } catch (error) {
      alert(`Registration failed: ${error}`);
      setStatus("Disconnected");
    }
  }

  function handleLogout() {
    localStorage.removeItem("ylogin");
    localStorage.removeItem("phpdisk_info");
    localStorage.removeItem("phone");
    setStatus("Disconnected");
    setUsername("");
  }

  // --- UPLOAD MANAGER ---
  useEffect(() => {
    const processQueue = async () => {
      // 1. Prevent the 1-second interval from piling up if a sync is running
      if (isSyncing.current) return;

      let active = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
      const pendingOrRunning = active.filter((t: any) => !t.isGroup && (t.status === "Queued" || t.status === "Running"));
      
      // 2. BOOKEND PUSH: The queue just finished its last item. Push the batch to the cloud!
      if (pendingOrRunning.length === 0) {
        if (isUploadingBatch.current) {
          isUploadingBatch.current = false;
          isSyncing.current = true;
          console.log("[SYNC] Upload batch finished. Pushing to cloud...");
          await invoke("vfs_sync_push").catch(e => console.warn("Sync push skipped:", e));
          isSyncing.current = false;
          keepScreenOn(false).catch(() => {});
          window.dispatchEvent(new CustomEvent("TASK_END")); // Refresh UI globally
        }
        return;
      }

      const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
      const limit = config.concurrentUploads || 2;

      const runningCount = active.filter((t: any) => !t.isGroup && t.status === "Running").length;
      if (runningCount >= limit) return;

      const nextTaskIndex = active.findIndex((t: any) => !t.isGroup && t.status === "Queued");
      if (nextTaskIndex === -1) return;

      // 3. BOOKEND PULL: A new batch is starting. Pull latest cloud state before uploading!
      if (!isUploadingBatch.current) {
        isUploadingBatch.current = true;
        isSyncing.current = true;
        keepScreenOn(true).catch(() => {});
        console.log("[SYNC] Upload batch starting. Pulling from cloud...");
        await invoke("vfs_sync_pull").catch(e => console.warn("Sync pull skipped:", e));
        isSyncing.current = false;
        return processQueue(); // Re-evaluate queue after pulling
      }

      const task = active[nextTaskIndex];
      active[nextTaskIndex].status = "Running";
      localStorage.setItem("heriheri_active", JSON.stringify(active));
      window.dispatchEvent(new CustomEvent("TASK_START"));

      invoke("vfs_upload_file", { 
        filePath: task.filePath, taskId: task.id, targetPid: task.targetPid, resumeFolder: task.resumeFolder || "", resumeChunk: task.resumeChunk || 0 
      })
      .then(() => finishTask(task.id, task.name, "Upload", "Success", undefined, task.groupId))
      .catch((err) => {
        const errMsg = String(err);
        if (errMsg.startsWith("EXISTS:")) {
          finishTask(task.id, task.name, "Upload", "Skipped", `Exists: ${errMsg.replace("EXISTS:", "")}`, task.groupId);
        } else if (errMsg.includes("PAUSED")) {
           let folder = "", chunk = "0";
           if (errMsg.startsWith("PAUSED:")) [folder, chunk] = errMsg.split(":")[1].split("|");
           let activeList = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
           const i = activeList.findIndex((t: any) => t.id === task.id);
           if (i > -1) {
              activeList[i].status = "Paused";
              activeList[i].resumeFolder = folder;
              activeList[i].resumeChunk = parseInt(chunk);
              localStorage.setItem("heriheri_active", JSON.stringify(activeList));
              window.dispatchEvent(new CustomEvent("TASK_END")); 
           }
        } else if (errMsg.includes("CANCELLED")) {
           let activeList = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
           activeList = activeList.filter((t: any) => t.id !== task.id);
           localStorage.setItem("heriheri_active", JSON.stringify(activeList));
           window.dispatchEvent(new CustomEvent("TASK_END"));
        } else {
           finishTask(task.id, task.name, "Upload", "Failed", errMsg, task.groupId);
        }
      });
    };

    const interval = setInterval(processQueue, 1000);
    window.addEventListener("TASK_START", processQueue);
    window.addEventListener("TASK_END", processQueue);
    return () => { clearInterval(interval); window.removeEventListener("TASK_START", processQueue); window.removeEventListener("TASK_END", processQueue); };
  }, []);

  function finishTask(id: string, name: string, type: string, status: string, error?: string, groupId?: string) {
    let active = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
    active = active.filter((t: any) => t.id !== id);
    const finished = JSON.parse(localStorage.getItem("heriheri_finished") || "[]");

    if (groupId) {
      const gIdx = active.findIndex((t: any) => t.id === groupId);
      if (gIdx > -1) {
        active[gIdx].finishedItems += 1;
        if (active[gIdx].finishedItems >= active[gIdx].totalItems) {
          const grp = active[gIdx];
          active = active.filter((t: any) => t.id !== groupId);
          finished.push({ id: groupId, name: grp.name, type: "Upload", status: "Success", time: Date.now() });
        }
      }
    } else {
      finished.push({ id, name, type, status, error, time: Date.now() });
    }

    if (status === "Success" && !groupId) {
      const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
      const shouldNotify = config.notifyUpload !== false;
      const playSound = config.notifySound === true;
      
      if (shouldNotify) {
        triggerSystemNotification("Upload Complete", `${name} has finished uploading.`, playSound);
      }
    }

    localStorage.setItem("heriheri_active", JSON.stringify(active));
    localStorage.setItem("heriheri_finished", JSON.stringify(finished));
    window.dispatchEvent(new CustomEvent("TASK_END"));
  }

  // --- DOWNLOAD MANAGER ---
  useEffect(() => {
    const processDownQueue = () => {
      let active = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");

      const pendingOrRunning = active.filter((t: any) => !t.isGroup && (t.status === "Queued" || t.status === "Running"));
      if (pendingOrRunning.length === 0) {
        keepScreenOn(false).catch(() => {});
        return;
      } else {
        keepScreenOn(true).catch(() => {});
      }

      const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
      const limit = config.concurrentDownloads || 2;
      
      const runningCount = active.filter((t: any) => !t.isGroup && t.status === "Running").length;
      if (runningCount >= limit) return;

      const nextTaskIndex = active.findIndex((t: any) => !t.isGroup && t.status === "Queued");
      if (nextTaskIndex === -1) return;

      const task = active[nextTaskIndex];
      active[nextTaskIndex].status = "Running";
      localStorage.setItem("heriheri_down_active", JSON.stringify(active));
      window.dispatchEvent(new CustomEvent("DOWN_TASK_START"));

      invoke("vfs_download_file", { 
        taskId: task.id, 
        vfsId: task.vfsId || 0,          
        shareCode: task.shareCode || "", 
        localPath: task.localPath, 
        resumeOffset: task.resumeOffset || 0, 
        totalSize: task.totalSize || 0
      })
      .then(() => finishDownTask(task.id, task.name, "Download", "Success", undefined, task.groupId))
      .catch((err) => {
        const errMsg = String(err);
        if (errMsg.includes("PAUSED")) {
           let offset = "0";
           if (errMsg.startsWith("PAUSED:")) offset = errMsg.split(":")[1];
           let activeList = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");
           const i = activeList.findIndex((t: any) => t.id === task.id);
           if (i > -1) {
              activeList[i].status = "Paused";
              activeList[i].resumeOffset = parseInt(offset);
              localStorage.setItem("heriheri_down_active", JSON.stringify(activeList));
              window.dispatchEvent(new CustomEvent("DOWN_TASK_END")); 
           }
        } else if (errMsg.includes("CANCELLED")) {
           let activeList = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");
           activeList = activeList.filter((t: any) => t.id !== task.id);
           localStorage.setItem("heriheri_down_active", JSON.stringify(activeList));
           window.dispatchEvent(new CustomEvent("DOWN_TASK_END"));
        } else {
           console.error(`[Download Failed] Task: ${task.name} | Error:`, errMsg);
           finishDownTask(task.id, task.name, "Download", "Failed", errMsg, task.groupId);
        }
      });
    };

    const interval = setInterval(processDownQueue, 1000);
    window.addEventListener("DOWN_TASK_START", processDownQueue);
    window.addEventListener("DOWN_TASK_END", processDownQueue);
    return () => { clearInterval(interval); window.removeEventListener("DOWN_TASK_START", processDownQueue); window.removeEventListener("DOWN_TASK_END", processDownQueue); };
  }, []);

  function finishDownTask(id: string, name: string, type: string, status: string, error?: string, groupId?: string) {
    let active = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");
    active = active.filter((t: any) => t.id !== id);
    const finished = JSON.parse(localStorage.getItem("heriheri_finished") || "[]");

    if (groupId) {
      const gIdx = active.findIndex((t: any) => t.id === groupId);
      if (gIdx > -1) {
        active[gIdx].finishedItems += 1;
        if (active[gIdx].finishedItems >= active[gIdx].totalItems) {
          const grp = active[gIdx];
          active = active.filter((t: any) => t.id !== groupId);
          finished.push({ id: groupId, name: grp.name, type: "Download", status: "Success", time: Date.now() });
        }
      }
    } else {
      finished.push({ id, name, type, status, error, time: Date.now() });
    }

    if (status === "Success" && !groupId) {
      const config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
      const shouldNotify = config.notifyDownload !== false;
      const playSound = config.notifySound === true;
      
      if (shouldNotify) {
        triggerSystemNotification("Download Complete", `${name} has finished downloading.`, playSound);
      }
    }

    localStorage.setItem("heriheri_down_active", JSON.stringify(active));
    localStorage.setItem("heriheri_finished", JSON.stringify(finished));
    window.dispatchEvent(new CustomEvent("DOWN_TASK_END"));
    window.dispatchEvent(new CustomEvent("TASK_END"));
  }

  const renderContent = () => {
    if (activeTab === "home") return <Home status={status} />;
    if (activeTab === "transfer") return <Transfer />;
    if (activeTab === "settings") return <Settings isMobile={isMobile} AppLogo={AppLogo} AppAuth={AppAuth} />;
    if (activeTab === "rent") return <Rent />;
    return <Home status={status} />;
  };

  const AppLogo = (
    <div style={{...styles.logoContainer, marginBottom: isMobile ? 0 : "30px"}}>
      <h1 style={{...styles.logoText, fontSize: isMobile ? "16px" : "20px"}}>HERIHERI</h1>
      <div style={{ ...styles.badge, position: "relative", cursor: "pointer" }} onClick={() => updateAvailable ? setShowUpdateModal(true) : alert(t("You are on the latest version!"))}>
        V{appVersion || "..."}
        {updateAvailable && <div style={styles.redDot} />}
      </div>
    </div>
  );

  const AppAuth = (
    <div style={isMobile ? styles.mobileAuthCard : styles.authCard}>
      {status === "Connected" ? (
        <div style={{...styles.profileContainer, gap: isMobile ? "8px" : "12px"}}>
          <img src="https://up.woozooo.com/images/u.gif" alt="avatar" style={styles.avatar} />
          {!isMobile && (
            <div style={{ flex: 1 }}>
              <div style={styles.username}>{username}</div>
              <div style={styles.statusText}>{t("ONLINE")}</div>
            </div>
          )}
          <button style={styles.logoutBtn} onClick={handleLogout} title="Logout">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M9 21H3V3h6M16 17l5-5-5-5M21 12H9"/></svg>
          </button>
        </div>
      ) : (
        <div style={{ display: "flex", gap: "8px", flexDirection: isMobile ? "row" : "column" }}>
          <button style={{...styles.primaryButton, padding: isMobile ? "6px 12px" : "10px"}} onClick={() => setShowLogin(true)}>{t("Login")}</button>
          {!isMobile && <button style={styles.secondaryButton} onClick={() => setShowRegister(true)}>{t("Register")}</button>}
        </div>
      )}
    </div>
  );

  const AppNav = (
    <nav style={isMobile ? styles.mobileNavMenu : styles.navMenu}>
      <div style={activeTab === "home" ? (isMobile ? styles.mobileNavItemActive : styles.navItemActive) : (isMobile ? styles.mobileNavItem : styles.navItem)} onClick={() => setActiveTab("home")}>
        <svg style={styles.navIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M2 3h6l2 3h12v15H2z"/></svg>
        {!isMobile && t("All Files")}
      </div>
      {!isMobile && <hr style={styles.divider} />}
      <div style={activeTab === "transfer" ? (isMobile ? styles.mobileNavItemActive : styles.navItemActive) : (isMobile ? styles.mobileNavItem : styles.navItem)} onClick={() => setActiveTab("transfer")}>
        <svg style={styles.navIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><path d="M21 15v4H3v-4M7 10l5 5 5-5M12 15V3"/></svg>
        {!isMobile && t("Transfer")}
      </div>
      <div style={activeTab === "rent" ? (isMobile ? styles.mobileNavItemActive : styles.navItemActive) : (isMobile ? styles.mobileNavItem : styles.navItem)} onClick={() => setActiveTab("rent")}>
        <svg style={styles.navIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><polyline points="16 3 21 8 16 13"/><line x1="21" y1="8" x2="9" y2="8"/><polyline points="8 21 3 16 8 11"/><line x1="3" y1="16" x2="15" y2="16"/></svg>
        {!isMobile && t("Rent")}
      </div>
      {!isMobile && <hr style={styles.divider} />}
      <div style={activeTab === "settings" ? (isMobile ? styles.mobileNavItemActive : styles.navItemActive) : (isMobile ? styles.mobileNavItem : styles.navItem)} onClick={() => setActiveTab("settings")}>
        <svg style={styles.navIcon} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="square" strokeLinejoin="miter"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
        {!isMobile && t("Settings")}
      </div>
    </nav>
  );

  return (
    <div style={{ ...styles.appContainer, flexDirection: "column" }}>

      {/* Main Responsive Grid */}
      <div style={{ display: "flex", flex: 1, flexDirection: isMobile ? "column" : "row", overflow: "hidden" }}>
        
        {/* Desktop Sidebar */}
        {!isMobile && (
          <aside style={styles.sidebar}>
            {AppLogo}
            {AppNav}
            {AppAuth}
          </aside>
        )}

        {/* Content Area */}
        <main style={{...styles.mainContent, padding: isMobile ? "16px" : "32px 48px"}}>
          {renderContent()}
        </main>
      </div>

      {/* Mobile Bottom Nav */}
      {isMobile && (
        <div style={styles.mobileBottomBar}>
          {AppNav}
        </div>
      )}

      {/* --- MODALS --- */}
      {showLogin && (
        <div style={styles.modalOverlay} onClick={() => setShowLogin(false)}>
          <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
            <h3 style={styles.modalTitle}>Login to Lanzou</h3>
            <form onSubmit={handleLoginSubmit} style={{ display: "flex", flexDirection: "column", gap: "16px", marginTop: "8px" }}>
              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Phone Number")}</label>
                <input style={styles.input} required
                  onChange={(e) => setLoginForm({ ...loginForm, phone: e.target.value })} />
              </div>
              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Password")}</label>
                <input style={styles.input} type="password" required
                  onChange={(e) => setLoginForm({ ...loginForm, password: e.target.value })} />
              </div>
              <button type="submit" style={{...styles.primaryButton, marginTop: "8px"}}>{t("Sign In")}</button>
            </form>
          </div>
        </div>
      )}

      {showRegister && (
        <div style={styles.modalOverlay} onClick={() => setShowRegister(false)}>
          <div style={styles.modalBox} onClick={(e) => e.stopPropagation()}>
            <h3 style={styles.modalTitle}>{t("Register Account")}</h3>
            
            <form onSubmit={handleRegisterSubmit} style={{ display: "flex", flexDirection: "column", gap: "16px", marginTop: "8px" }}>
              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Phone Number")}</label>
                <input style={styles.input} required
                  onChange={(e) => setRegForm({ ...regForm, phone: e.target.value })} />
              </div>

              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Verification Code")}</label>
                <div style={{ display: "flex", gap: "8px" }}>
                  <input style={{...styles.input, flex: 1}} required
                    onChange={(e) => setRegForm({ ...regForm, code: e.target.value })} />
                  <button 
                    type="button" 
                    onClick={handleRequestSMS} 
                    disabled={countdown > 0}
                    style={{...styles.secondaryButton, width: "100px", padding: "0"}}
                  >
                    {countdown > 0 ? `${countdown}s` : "Get Code"}
                  </button>
                </div>
              </div>

              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Password")}</label>
                <input style={styles.input} type="password" required
                  onChange={(e) => setRegForm({ ...regForm, password: e.target.value })} />
              </div>

              <div style={styles.inputGroup}>
                <label style={styles.inputLabel}>{t("Confirm Password")}</label>
                <input style={styles.input} type="password" required
                  onChange={(e) => setRegForm({ ...regForm, confirm: e.target.value })} />
              </div>

              <div style={{ display: "flex", gap: "10px", marginTop: "8px" }}>
                <button type="button" style={{...styles.secondaryButton, flex: 1}} onClick={() => setShowRegister(false)}>{t("Cancel")}</button>
                <button type="submit" style={{...styles.primaryButton, flex: 1}}>{t("Register")}</button>
              </div>
            </form>
          </div>
        </div>
      )}

      {showUpdateModal && updateAvailable && (
        <div style={styles.modalOverlay} onClick={() => setShowUpdateModal(false)}>
          <div style={{...styles.modalBox, width: "480px"}} onClick={(e) => e.stopPropagation()}>
            <h3 style={styles.modalTitle}>{t("Update Available")}</h3>
            
            <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
              <div style={styles.inputLabel}>{t("Current Version")}: V{appVersion}</div>
              <div style={styles.inputLabel}>{t("Latest Version")}: {updateAvailable.version}</div>
            </div>

            <div style={styles.inputGroup}>
              <label style={styles.inputLabel}>{t("Release Notes")}</label>
              <textarea 
                style={{...styles.input, flex: 1, minHeight: "140px", fontSize: "12px", resize: "none", backgroundColor: "#f9fafb", lineHeight: "1.5"}} 
                readOnly 
                value={updateAvailable.body || t("No changelog provided.")} 
              />
            </div>

            <div style={styles.modalActions}>
              <button 
                style={styles.secondaryButton} 
                onClick={() => setShowUpdateModal(false)}
              >
                {t("Cancel")}
              </button>
              <button 
                style={styles.primaryButton} 
                onClick={async () => {
                  await openBrowser(updateAvailable.url);
                  setShowUpdateModal(false);
                }}
              >
                {t("Download Update")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

const styles: { [key: string]: React.CSSProperties } = {
  appContainer: { display: "flex", height: "100vh", width: "100vw", backgroundColor: "#f3f4f6", fontFamily: "Inter, sans-serif", margin: 0 },
  sidebar: { width: "260px", backgroundColor: "#ffffff", borderRight: "1px solid #111827", display: "flex", flexDirection: "column", padding: "24px", zIndex: 10 },
  logoContainer: { display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: "30px" },
  logoText: { margin: 0, fontSize: "20px", fontWeight: "900", color: "#111827", letterSpacing: "1px" },
  badge: { backgroundColor: "#111827", color: "#ffffff", padding: "4px 8px", borderRadius: "0", fontSize: "11px", fontWeight: "700", border: "1px solid #111827" },
  navMenu: { display: "flex", flexDirection: "column", gap: "8px", flex: 1 },
  navItem: { display: "flex", alignItems: "center", gap: "12px", padding: "10px 12px", borderRadius: "0", cursor: "pointer", color: "#4b5563", fontSize: "12px", fontWeight: "700", textTransform: "uppercase", letterSpacing: "1px", transition: "background 0.2s, color 0.2s" },
  navItemActive: { display: "flex", alignItems: "center", gap: "12px", padding: "10px 12px", borderRadius: "0", cursor: "pointer", color: "#ffffff", backgroundColor: "#111827", fontSize: "12px", fontWeight: "700", textTransform: "uppercase", letterSpacing: "1px" },
  navIcon: { width: "16px", height: "16px" },
  divider: { border: "none", borderTop: "1px solid #e5e7eb", margin: "8px 0" },
  authCard: { backgroundColor: "#ffffff", padding: "16px", borderRadius: "0", border: "1px solid #111827", marginTop: "auto", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)" },
  profileContainer: { display: "flex", alignItems: "center", gap: "12px" },
  avatar: { width: "40px", height: "40px", borderRadius: "0", border: "2px solid #111827" },
  username: { fontSize: "13px", fontWeight: "700", color: "#111827", textTransform: "uppercase" },
  statusText: { fontSize: "10px", color: "#111827", fontWeight: "700", letterSpacing: "1px" },
  logoutBtn: { background: "transparent", border: "1px solid transparent", cursor: "pointer", color: "#111827", display: "flex", alignItems: "center", justifyContent: "center", padding: "6px", transition: "border 0.2s" },
  primaryButton: { backgroundColor: "#111827", color: "#ffffff", padding: "10px", borderRadius: "0", border: "1px solid #111827", fontSize: "12px", fontWeight: "700", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px" },
  secondaryButton: { backgroundColor: "#ffffff", color: "#111827", padding: "10px", borderRadius: "0", border: "1px solid #111827", fontSize: "12px", fontWeight: "700", cursor: "pointer", textTransform: "uppercase", letterSpacing: "1px" },
  mainContent: { flex: 1, padding: "32px 48px", overflowY: "auto" },

  modalOverlay: { position: "fixed", top: 0, left: 0, right: 0, bottom: 0, backgroundColor: "rgba(255, 255, 255, 0.9)", display: "flex", justifyContent: "center", alignItems: "center", zIndex: 999 },
  modalBox: { backgroundColor: "#ffffff", padding: "32px", borderRadius: "0", border: "2px solid #111827", width: "360px", boxShadow: "8px 8px 0px 0px rgba(17, 24, 39, 1)", display: "flex", flexDirection: "column", gap: "24px" },
  modalTitle: { margin: 0, fontSize: "16px", fontWeight: "800", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" }, 
  inputGroup: { display: "flex", flexDirection: "column", gap: "8px" },
  inputLabel: { fontSize: "10px", fontWeight: "700", color: "#111827", textTransform: "uppercase", letterSpacing: "1px" },
  input: { padding: "10px 12px", backgroundColor: "#ffffff", color: "#111827", border: "1px solid #111827", borderRadius: "0", fontSize: "13px", outline: "none" },
  redDot: { position: "absolute", top: "-4px", right: "-4px", width: "8px", height: "8px", backgroundColor: "#ef4444", borderRadius: "50%", border: "2px solid #ffffff" },

  mobileHeader: { display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px 16px", backgroundColor: "#ffffff", borderBottom: "1px solid #111827", zIndex: 10 },
  mobileBottomBar: { backgroundColor: "#ffffff", borderTop: "1px solid #111827", zIndex: 10, padding: "12px 0", paddingBottom: "calc(env(safe-area-inset-bottom, 0px) + 16px)" },
  mobileNavMenu: { display: "flex", flexDirection: "row", justifyContent: "space-around", width: "100%" },
  mobileNavItem: { display: "flex", alignItems: "center", justifyContent: "center", padding: "8px", color: "#9ca3af", cursor: "pointer", transition: "color 0.2s" },
  mobileNavItemActive: { display: "flex", alignItems: "center", justifyContent: "center", padding: "8px", color: "#111827", cursor: "pointer" },
  mobileAuthCard: { display: "flex", alignItems: "center", gap: "12px" },
};