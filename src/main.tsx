import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter, Routes, Route } from "react-router-dom";
import App from "./App";
import MediaPlayer from "./Windows/MediaPlayer";
import "./i18n";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <HashRouter>
      <Routes>
        {/* The main app UI (Sidebar, Files, etc.) */}
        <Route path="/" element={<App />} />
        
        {/* The isolated Video Player Window */}
        <Route path="/player" element={<MediaPlayer />} />
      </Routes>
    </HashRouter>
  </React.StrictMode>,
);