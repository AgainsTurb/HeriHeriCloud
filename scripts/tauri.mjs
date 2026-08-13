import { existsSync } from "node:fs";
import { delimiter, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriCli = resolve(projectRoot, "node_modules", "@tauri-apps", "cli", "tauri.js");
const args = process.argv.slice(2);
const targetDescription = [
  ...args,
  process.env.TAURI_ENV_PLATFORM,
  process.env.TAURI_ENV_PLATFORM_TYPE,
  process.env.TARGET,
  process.env.CARGO_BUILD_TARGET,
].filter(Boolean).join(" ");
const isMobileTarget = /android|androideabi|ios|iphoneos|iphonesimulator/i.test(targetDescription);

function environmentValue(environment, name) {
  const key = Object.keys(environment).find((candidate) => candidate.toLowerCase() === name.toLowerCase());
  return key ? environment[key] : undefined;
}

function setWindowsEnvironmentValue(environment, name, value) {
  for (const key of Object.keys(environment)) {
    if (key.toLowerCase() === name.toLowerCase()) delete environment[key];
  }
  environment[name] = value;
}

function validWindowsRoot(root) {
  return Boolean(root) &&
    existsSync(join(root, "bin", "gstreamer-1.0-0.dll")) &&
    existsSync(join(root, "lib", "gstreamer-1.0")) &&
    existsSync(join(root, "libexec", "gstreamer-1.0", "gst-plugin-scanner.exe"));
}

function windowsGStreamerRoot(environment) {
  const configured = environmentValue(environment, "GSTREAMER_1_0_ROOT_MSVC_X86_64");
  const local = resolve(projectRoot, ".gstreamer", "1.0", "msvc_x86_64");
  const x86Root = environmentValue(environment, "GSTREAMER_1_0_ROOT_MSVC_X86");
  const sibling = x86Root ? resolve(x86Root, "..", "msvc_x86_64") : undefined;
  const pathRoots = (environmentValue(environment, "Path") || "")
    .split(delimiter)
    .filter((entry) => /msvc_x86_64[\\/]bin[\\/]?$/i.test(entry))
    .map((entry) => resolve(entry, ".."));
  const conventional = [
    "C:\\gstreamer\\1.0\\msvc_x86_64",
    join(process.env.ProgramFiles || "C:\\Program Files", "gstreamer", "1.0", "msvc_x86_64"),
  ];

  return [local, configured, sibling, ...pathRoots, ...conventional].find(validWindowsRoot);
}

function configureWindowsGStreamer(environment) {
  const root = windowsGStreamerRoot(environment);
  if (!root) {
    throw new Error(
      "64-bit GStreamer was not found. Run 'npm run setup:gstreamer:windows' or set " +
      "GSTREAMER_1_0_ROOT_MSVC_X86_64 before starting the desktop app.",
    );
  }

  const bin = join(root, "bin");
  const plugins = join(root, "lib", "gstreamer-1.0");
  const scanner = join(root, "libexec", "gstreamer-1.0", "gst-plugin-scanner.exe");
  const cleanPath = (environmentValue(environment, "Path") || "")
    .split(delimiter)
    .filter((entry) => entry && !/[\\/]gstreamer[\\/]1\.0[\\/](?:msvc_)?x86[\\/]bin[\\/]?$/i.test(entry));

  setWindowsEnvironmentValue(environment, "GSTREAMER_1_0_ROOT_MSVC_X86_64", root);
  setWindowsEnvironmentValue(environment, "GST_PLUGIN_PATH", plugins);
  setWindowsEnvironmentValue(environment, "GST_PLUGIN_SYSTEM_PATH_1_0", plugins);
  setWindowsEnvironmentValue(environment, "GST_PLUGIN_SCANNER_1_0", scanner);
  setWindowsEnvironmentValue(environment, "PKG_CONFIG_PATH", [
    join(root, "lib", "pkgconfig"),
    join(root, "share", "pkgconfig"),
    environmentValue(environment, "PKG_CONFIG_PATH"),
  ].filter(Boolean).join(delimiter));
  // Windows treats names case-insensitively, but Node can receive both Path and PATH.
  // Passing both to CreateProcess is ambiguous and previously hid cargo.exe on some shells.
  setWindowsEnvironmentValue(environment, "Path", [bin, ...cleanPath].join(delimiter));
  console.log(`[HeriHeriCloud] Using 64-bit GStreamer from ${root}`);
}

const environment = { ...process.env };
if (process.platform === "win32" && !isMobileTarget) configureWindowsGStreamer(environment);

const result = spawnSync(process.execPath, [tauriCli, ...args], {
  cwd: projectRoot,
  env: environment,
  stdio: "inherit",
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
