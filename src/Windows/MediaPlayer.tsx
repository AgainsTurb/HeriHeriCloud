import React, { useEffect, useState, useRef } from "react";

export default function MediaPlayer() {
  const [streamUrl, setStreamUrl] = useState("");
  const [title, setTitle] = useState("");
  const [isAudio, setIsAudio] = useState(false);
  
  // --- Professional Player States ---
  const [isBuffering, setIsBuffering] = useState(true);
  const [playerError, setPlayerError] = useState<string | null>(null);
  const [codecWarning, setCodecWarning] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    const hash = window.location.hash;
    const queryString = hash.includes('?') ? hash.substring(hash.indexOf('?')) : '';
    const params = new URLSearchParams(queryString);
    
    const url = params.get("url") || "";
    const mediaTitle = params.get("title") || "Media Player";
    
    setStreamUrl(url);
    setTitle(mediaTitle);
    setIsAudio(params.get("isAudio") === "true");

    // --- PROACTIVE CODEC PROBING ---
    // If the title implies HEVC/H.265, check if the system engine supports it
    const titleLower = mediaTitle.toLowerCase();
    if (titleLower.includes("hevc") || titleLower.includes("h265") || titleLower.includes("x265")) {
      const canPlayHEVC = 
        MediaSource.isTypeSupported('video/mp4; codecs="hev1.1.6.L93.B0"') || 
        MediaSource.isTypeSupported('video/mp4; codecs="hvc1.1.6.L93.B0"');
      
      if (!canPlayHEVC) {
        setCodecWarning("⚠️ System lacks HEVC/H.265 hardware decoding. You may hear audio but see a black screen. Please install the HEVC extension on your OS.");
      }
    }
  }, []);

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h2 style={styles.title}>{title}</h2>
      </div>
      
      <div style={styles.playerWrapper}>
        {/* --- DYNAMIC WARNING OVERLAY --- */}
        {codecWarning && (
          <div style={styles.warningBanner}>{codecWarning}</div>
        )}

        {/* --- FATAL ERROR OVERLAY --- */}
        {playerError && (
          <div style={styles.errorOverlay}>
            <div style={styles.errorBox}>
              <h3 style={{ margin: "0 0 8px 0" }}>Playback Error</h3>
              <p style={{ margin: 0 }}>{playerError}</p>
            </div>
          </div>
        )}

        {/* --- LOADING SPINNER (Tied to Rust Proxy Chunks) --- */}
        {isBuffering && !playerError && (
          <div style={styles.spinnerOverlay}>
            <div style={styles.spinner}></div>
            <div style={{ marginTop: "12px", fontSize: "12px", fontWeight: 700, letterSpacing: "1px" }}>BUFFERING...</div>
          </div>
        )}

        {streamUrl && (
          isAudio ? (
            <audio 
              controls 
              autoPlay 
              src={streamUrl} 
              style={styles.audioPlayer}
              onWaiting={() => setIsBuffering(true)}
              onPlaying={() => setIsBuffering(false)}
              onError={(e) => setPlayerError("Audio decoding failed or stream was interrupted.")}
            />
          ) : (
            <video 
              ref={videoRef}
              controls 
              autoPlay 
              src={streamUrl} 
              style={styles.videoPlayer}
              onWaiting={() => setIsBuffering(true)}
              onPlaying={() => setIsBuffering(false)}
              onCanPlay={() => setIsBuffering(false)}
              onError={(e) => {
                const target = e.target as HTMLVideoElement;
                if (target.error?.code === 3) {
                  setPlayerError("Decode Error: The video chunk was corrupted or uses an unsupported codec.");
                } else if (target.error?.code === 4) {
                  setPlayerError("Network Error: The Rust proxy dropped the connection.");
                } else {
                  setPlayerError(`Player Error Code: ${target.error?.code}`);
                }
              }}
            />
          )
        )}
      </div>
    </div>
  );
}

const styles: { [key: string]: React.CSSProperties } = {
  container: { display: "flex", flexDirection: "column", height: "100vh", backgroundColor: "#ffffff", border: "2px solid #111827", boxSizing: "border-box" },
  header: { padding: "16px", backgroundColor: "#111827", color: "#ffffff", borderBottom: "2px solid #111827", zIndex: 10 },
  title: { margin: 0, fontSize: "14px", fontWeight: "800", textTransform: "uppercase", letterSpacing: "1px", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" },
  playerWrapper: { flex: 1, display: "flex", justifyContent: "center", alignItems: "center", backgroundColor: "#f3f4f6", position: "relative", overflow: "hidden" },
  videoPlayer: { width: "100%", height: "100%", maxHeight: "100%", outline: "none", backgroundColor: "#000" },
  audioPlayer: { width: "100%", maxWidth: "500px", outline: "none", border: "2px solid #111827", boxShadow: "4px 4px 0px 0px rgba(17, 24, 39, 1)", zIndex: 5 },
  warningBanner: { position: "absolute", top: 0, left: 0, right: 0, backgroundColor: "#fef08a", color: "#9a3412", padding: "12px 16px", fontSize: "12px", fontWeight: 600, borderBottom: "1px solid #ca8a04", zIndex: 10, textAlign: "center" },
  errorOverlay: { position: "absolute", inset: 0, backgroundColor: "rgba(0, 0, 0, 0.8)", display: "flex", justifyContent: "center", alignItems: "center", zIndex: 20 },
  errorBox: { backgroundColor: "#ffffff", border: "2px solid #ef4444", borderTopWidth: "6px", padding: "24px", maxWidth: "400px", color: "#111827", boxShadow: "8px 8px 0px 0px rgba(17, 24, 39, 1)" },
  spinnerOverlay: { position: "absolute", inset: 0, display: "flex", flexDirection: "column", justifyContent: "center", alignItems: "center", color: "#ffffff", zIndex: 5, pointerEvents: "none" },
  spinner: { width: "40px", height: "40px", border: "4px solid rgba(255,255,255,0.3)", borderTop: "4px solid #ffffff", borderRadius: "50%", animation: "spin 1s linear infinite" }
};

// Add CSS animation for the spinner
const css = `
@keyframes spin { 0% { transform: rotate(0deg); } 100% { transform: rotate(360deg); } }
`;
if (typeof document !== "undefined") {
  const style = document.createElement("style");
  style.textContent = css;
  document.head.appendChild(style);
}