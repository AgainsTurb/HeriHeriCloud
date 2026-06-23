import React, { useEffect, useState } from "react";

export default function ImageView() {
  const [streamUrl, setStreamUrl] = useState("");
  const [title, setTitle] = useState("");

  useEffect(() => {
    const hash = window.location.hash;
    const queryString = hash.includes('?') ? hash.substring(hash.indexOf('?')) : '';
    const params = new URLSearchParams(queryString);
    
    setStreamUrl(params.get("url") || "");
    setTitle(params.get("title") || "Image Viewer");
  }, []);

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h2 style={styles.title}>{title}</h2>
      </div>
      
      <div style={styles.viewerWrapper}>
        {streamUrl ? (
          <img 
            src={streamUrl} 
            alt={title} 
            style={styles.image} 
          />
        ) : (
          <div style={styles.loadingText}>LOADING IMAGE...</div>
        )}
      </div>
    </div>
  );
}

const styles: { [key: string]: React.CSSProperties } = {
  container: { display: "flex", flexDirection: "column", height: "100vh", backgroundColor: "#ffffff", border: "2px solid #111827", boxSizing: "border-box" },
  header: { padding: "16px", backgroundColor: "#111827", color: "#ffffff", borderBottom: "2px solid #111827", zIndex: 10 },
  title: { margin: 0, fontSize: "14px", fontWeight: "800", textTransform: "uppercase", letterSpacing: "1px", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
  viewerWrapper: { 
    flex: 1, 
    display: "flex", 
    justifyContent: "center", 
    alignItems: "center", 
    backgroundColor: "#e5e7eb", 
    position: "relative", 
    overflow: "hidden",
    // Checkerboard pattern for transparent images
    backgroundImage: "linear-gradient(45deg, #d1d5db 25%, transparent 25%), linear-gradient(-45deg, #d1d5db 25%, transparent 25%), linear-gradient(45deg, transparent 75%, #d1d5db 75%), linear-gradient(-45deg, transparent 75%, #d1d5db 75%)",
    backgroundSize: "20px 20px",
    backgroundPosition: "0 0, 0 10px, 10px -10px, -10px 0px"
  },
  image: { maxWidth: "100%", maxHeight: "100%", objectFit: "contain", border: "2px solid #111827", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)", backgroundColor: "#ffffff" },
  loadingText: { fontWeight: 700, fontSize: "12px", letterSpacing: "1px", color: "#111827", backgroundColor: "#ffffff", padding: "8px 16px", border: "2px solid #111827" }
};