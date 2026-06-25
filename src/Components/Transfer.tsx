import { useState } from "react";
import { useTranslation } from "react-i18next";

// Import your existing modular sub-pages
import Uploading from "./Uploading";
import Downloading from "./Downloading";
import Finished from "./Finished";

export default function Transfer() {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<"Uploading" | "Downloading" | "Finished">("Downloading");

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%" }}>
      {/* Neo-Brutalist Tab Header Switcher */}
      <header style={styles.header}>
        <div style={{ display: "flex", gap: "24px", alignItems: "center" }}>
          <h2 
            style={{
              ...styles.tabTitle, 
              color: activeTab === "Downloading" ? "#111827" : "#9ca3af", 
              borderBottom: activeTab === "Downloading" ? "2px solid #111827" : "2px solid transparent"
            }} 
            onClick={() => setActiveTab("Downloading")}
          >
            {t("Downloading")}
          </h2>
          <h2 
            style={{
              ...styles.tabTitle, 
              color: activeTab === "Uploading" ? "#111827" : "#9ca3af", 
              borderBottom: activeTab === "Uploading" ? "2px solid #111827" : "2px solid transparent"
            }} 
            onClick={() => setActiveTab("Uploading")}
          >
            {t("Uploading")}
          </h2>
          <h2 
            style={{
              ...styles.tabTitle, 
              color: activeTab === "Finished" ? "#111827" : "#9ca3af", 
              borderBottom: activeTab === "Finished" ? "2px solid #111827" : "2px solid transparent"
            }} 
            onClick={() => setActiveTab("Finished")}
          >
            {t("Finished")}
          </h2>
        </div>
      </header>

      {/* Main View Area plugging in the child components */}
      <div style={styles.viewArea}>
        {activeTab === "Downloading" && <Downloading />}
        {activeTab === "Uploading" && <Uploading />}
        {activeTab === "Finished" && <Finished />}
      </div>
    </div>
  );
}

// --------------------------------------------------------
// Styling Layout (Matches the Brutalist design language)
// --------------------------------------------------------
const styles: { [key: string]: React.CSSProperties } = {
  header: { 
    display: "flex", 
    justifyContent: "space-between", 
    alignItems: "flex-end", 
    marginBottom: "20px", 
    borderBottom: "2px solid #111827" 
  },
  tabTitle: { 
    margin: 0, 
    paddingBottom: "12px", 
    cursor: "pointer", 
    fontSize: "18px", 
    fontWeight: "800", 
    transition: "color 0.2s, border-color 0.2s", 
    textTransform: "uppercase", 
    letterSpacing: "1px" 
  },
  viewArea: { 
    flex: 1, 
    display: "flex", 
    flexDirection: "column", 
    minHeight: 0 // Prevents layout blowing out parent heights
  }
};