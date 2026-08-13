import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
// @ts-expect-error Node.js types are intentionally not installed in the frontend project.
import { fileURLToPath } from "node:url";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
// @ts-expect-error process is a nodejs global
const tauriTarget = [process.env.TAURI_ENV_PLATFORM, process.env.TAURI_ENV_PLATFORM_TYPE, process.env.TARGET, process.env.CARGO_BUILD_TARGET]
  .filter(Boolean)
  .join(" ");
const isMobileTarget = /android|androideabi|ios|iphoneos|iphonesimulator/i.test(tauriTarget);
const conceptDesktopEntry = fileURLToPath(new URL(
  isMobileTarget ? "./src/Components/ConceptDesktopEntry.mobile.tsx" : "./src/Components/ConceptDesktopEntry.desktop.ts",
  import.meta.url,
));

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],
  resolve: {
    alias: {
      "@concept-desktop": conceptDesktopEntry,
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 5173,
    strictPort: true,
    host: true,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. Rust sources and Cargo artifacts are watched by Tauri itself. Ignoring
      // Cargo's target tree also avoids Windows EBUSY errors on loaded DLLs.
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
}));
