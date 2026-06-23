import React, { useEffect, useState } from "react";

export default function DocView() {
  const [streamUrl, setStreamUrl] = useState("");
  const [title, setTitle] = useState("");
  const [isPdf, setIsPdf] = useState(false);

  useEffect(() => {
    const hash = window.location.hash;
    const queryString = hash.includes('?') ? hash.substring(hash.indexOf('?')) : '';
    const params = new URLSearchParams(queryString);
    
    const url = params.get("url") || "";
    const docTitle = params.get("title") || "Document Viewer";
    
    setStreamUrl(url);
    setTitle(docTitle);
    
    const ext = docTitle.split('.').pop()?.toLowerCase() || "";
    setIsPdf(ext === "pdf");
  }, []);

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h2 style={styles.title}>{title}</h2>
      </div>
      
      {!isPdf && (
        <div style={styles.warningBanner}>
          THIS IS NOT A PDF. BROWSER MAY PROMPT DOWNLOAD INSTEAD OF RENDERING IN-WINDOW.
        </div>
      )}

      <div style={styles.viewerWrapper}>
        {streamUrl ? (
          <iframe 
            src={streamUrl} 
            style={styles.iframe} 
            title={title}
          />
        ) : (
          <div style={styles.loadingText}>INITIALIZING DOCUMENT...</div>
        )}
      </div>
    </div>
  );
}

const styles: { [key: string]: React.CSSProperties } = {
  container: { display: "flex", flexDirection: "column", height: "100vh", backgroundColor: "#ffffff", border: "2px solid #111827", boxSizing: "border-box" },
  header: { padding: "16px", backgroundColor: "#111827", color: "#ffffff", borderBottom: "2px solid #111827", zIndex: 10 },
  title: { margin: 0, fontSize: "14px", fontWeight: "800", textTransform: "uppercase", letterSpacing: "1px", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
  warningBanner: { backgroundColor: "#fef08a", color: "#9a3412", padding: "8px 16px", fontSize: "11px", fontWeight: 700, borderBottom: "1px solid #ca8a04", textAlign: "center", letterSpacing: "0.5px" },
  viewerWrapper: { flex: 1, display: "flex", justifyContent: "center", alignItems: "center", backgroundColor: "#e5e7eb", position: "relative" },
  iframe: { width: "100%", height: "100%", border: "none", backgroundColor: "#ffffff" },
  loadingText: { fontWeight: 700, fontSize: "12px", letterSpacing: "1px", color: "#111827", backgroundColor: "#ffffff", padding: "8px 16px", border: "2px solid #111827" }
};