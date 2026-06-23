import React, { useEffect, useState } from "react";

export default function TextView() {
  const [title, setTitle] = useState("");
  const [textContent, setTextContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const hash = window.location.hash;
    const queryString = hash.includes('?') ? hash.substring(hash.indexOf('?')) : '';
    const params = new URLSearchParams(queryString);
    
    const url = params.get("url") || "";
    setTitle(params.get("title") || "Text Viewer");

    if (url) {
      fetch(url)
        .then(res => {
          if (!res.ok) throw new Error(`HTTP ${res.status}`);
          return res.text();
        })
        .then(text => setTextContent(text))
        .catch(err => setError(String(err)));
    }
  }, []);

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h2 style={styles.title}>{title}</h2>
      </div>
      
      <div style={styles.viewerWrapper}>
        {error ? (
          <div style={styles.errorBox}>FAILED TO LOAD TEXT: {error}</div>
        ) : textContent === null ? (
          <div style={styles.loadingBox}>FETCHING DOCUMENT...</div>
        ) : (
          <pre style={styles.textBlock}>
            {textContent}
          </pre>
        )}
      </div>
    </div>
  );
}

const styles: { [key: string]: React.CSSProperties } = {
  container: { display: "flex", flexDirection: "column", height: "100vh", backgroundColor: "#ffffff", border: "2px solid #111827", boxSizing: "border-box" },
  header: { padding: "16px", backgroundColor: "#111827", color: "#ffffff", borderBottom: "2px solid #111827", zIndex: 10 },
  title: { margin: 0, fontSize: "14px", fontWeight: "800", textTransform: "uppercase", letterSpacing: "1px", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
  viewerWrapper: { flex: 1, backgroundColor: "#f3f4f6", padding: "16px", overflow: "auto" },
  textBlock: { margin: 0, padding: "16px", backgroundColor: "#ffffff", border: "1px solid #111827", borderRadius: "0", fontSize: "13px", fontFamily: "Consolas, monospace", color: "#111827", whiteSpace: "pre-wrap", wordWrap: "break-word", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 0.1)" },
  loadingBox: { padding: "16px", fontWeight: 700, fontSize: "12px", border: "2px solid #111827", textAlign: "center", backgroundColor: "#ffffff" },
  errorBox: { padding: "16px", fontWeight: 700, fontSize: "12px", border: "2px solid #ef4444", color: "#ef4444", textAlign: "center", backgroundColor: "#fef2f2" }
};