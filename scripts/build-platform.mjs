import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriTarget = [
  process.env.TAURI_ENV_PLATFORM,
  process.env.TAURI_ENV_PLATFORM_TYPE,
  process.env.TARGET,
  process.env.CARGO_BUILD_TARGET,
].filter(Boolean).join(" ");
const isMobileTarget = /android|androideabi|ios|iphoneos|iphonesimulator/i.test(tauriTarget);

function runNodeScript(relativeScript, args) {
  const result = spawnSync(process.execPath, [resolve(projectRoot, relativeScript), ...args], {
    cwd: projectRoot,
    env: process.env,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

runNodeScript("node_modules/typescript/bin/tsc", ["--project", isMobileTarget ? "tsconfig.mobile.json" : "tsconfig.json"]);
runNodeScript("node_modules/vite/bin/vite.js", ["build"]);
