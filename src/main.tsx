import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter, Routes, Route } from "react-router-dom";
import App from "./App";
import MediaPlayer from "./Windows/MediaPlayer";
import ImageView from "./Windows/ImageView";
import TextView from "./Windows/TextView";
import DocView from "./Windows/DocView";
import "./i18n";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <HashRouter>
      <Routes>
        {/* The main app UI (Sidebar, Files, etc.) */}
        <Route path="/" element={<App />} />
        
        {/* The isolated Video Player Window */}
        <Route path="/player" element={<MediaPlayer />} />
        <Route path="/image" element={<ImageView />} />
        <Route path="/text" element={<TextView />} />
        <Route path="/doc" element={<DocView />} />
      </Routes>
    </HashRouter>
  </React.StrictMode>,
);