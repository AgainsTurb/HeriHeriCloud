import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { delimiter, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriCli = resolve(projectRoot, "node_modules", "@tauri-apps", "cli", "tauri.js");
const setupScript = resolve(projectRoot, "scripts", "setup-gstreamer.mjs");
const androidNdkVersion = "29.0.13846066";
const args = process.argv.slice(2);
const targetDescription = [
  ...args,
  process.env.TAURI_ENV_PLATFORM,
  process.env.TAURI_ENV_PLATFORM_TYPE,
  process.env.TARGET,
  process.env.CARGO_BUILD_TARGET,
].filter(Boolean).join(" ");
const isAndroidTarget = /android|androideabi/i.test(targetDescription);
const isIOSTarget = /\bios\b|iphoneos|iphonesimulator/i.test(targetDescription);

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

function runSetup(platform, extraArguments = []) {
  const result = spawnSync(process.execPath, [setupScript, "--platform", platform, ...extraArguments], {
    cwd: projectRoot,
    env: process.env,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Automatic GStreamer setup for ${platform} failed with code ${result.status}`);
}

function commandSucceeds(command, commandArguments) {
  const result = spawnSync(command, commandArguments, { cwd: projectRoot, env: process.env, stdio: "ignore" });
  return !result.error && result.status === 0;
}

function validWindowsRoot(root) {
  return Boolean(root) &&
    existsSync(join(root, "bin", "gstreamer-1.0-0.dll")) &&
    existsSync(join(root, "lib", "gstreamer-1.0")) &&
    existsSync(join(root, "libexec", "gstreamer-1.0", "gst-plugin-scanner.exe"));
}

function windowsArchitecture() {
  if (/aarch64|arm64/i.test(targetDescription) || process.arch === "arm64") return "arm64";
  if (/i686|ia32/i.test(targetDescription) || process.arch === "ia32") return "x86";
  return "x86_64";
}

function windowsEnvironmentVariable(architecture) {
  return `GSTREAMER_1_0_ROOT_MSVC_${architecture.toUpperCase()}`;
}

function windowsGStreamerRoot(environment, architecture) {
  const configured = environmentValue(environment, windowsEnvironmentVariable(architecture));
  const local = resolve(projectRoot, ".gstreamer", "1.0", `msvc_${architecture}`);
  const pathRoots = (environmentValue(environment, "Path") || "")
    .split(delimiter)
    .filter((entry) => new RegExp(`msvc_${architecture}[\\\\/]bin[\\\\/]?$`, "i").test(entry))
    .map((entry) => resolve(entry, ".."));
  const conventional = [
    `C:\\gstreamer\\1.0\\msvc_${architecture}`,
    join(process.env.ProgramFiles || "C:\\Program Files", "gstreamer", "1.0", `msvc_${architecture}`),
    join(process.env.LOCALAPPDATA || "", "Programs", "gstreamer", "1.0", `msvc_${architecture}`),
  ];
  return [local, configured, ...pathRoots, ...conventional].find(validWindowsRoot);
}

function configureWindowsGStreamer(environment) {
  const architecture = windowsArchitecture();
  let root = windowsGStreamerRoot(environment, architecture);
  if (!root) {
    console.log(`[HeriHeriCloud] GStreamer for Windows ${architecture} was not found; fetching the latest stable SDK.`);
    runSetup("windows", ["--arch", architecture]);
    root = windowsGStreamerRoot(environment, architecture);
  }
  if (!root) throw new Error(`GStreamer setup completed, but the Windows ${architecture} SDK could not be located`);

  const bin = join(root, "bin");
  const plugins = join(root, "lib", "gstreamer-1.0");
  const scanner = join(root, "libexec", "gstreamer-1.0", "gst-plugin-scanner.exe");
  const cleanPath = (environmentValue(environment, "Path") || "")
    .split(delimiter)
    .filter((entry) => entry && !/[\\/]gstreamer[\\/]1\.0[\\/](?:msvc_)?(?:x86|x86_64|arm64)[\\/]bin[\\/]?$/i.test(entry));

  setWindowsEnvironmentValue(environment, windowsEnvironmentVariable(architecture), root);
  setWindowsEnvironmentValue(environment, "GST_PLUGIN_PATH", plugins);
  setWindowsEnvironmentValue(environment, "GST_PLUGIN_SYSTEM_PATH_1_0", plugins);
  setWindowsEnvironmentValue(environment, "GST_PLUGIN_SCANNER_1_0", scanner);
  setWindowsEnvironmentValue(environment, "PKG_CONFIG_PATH", [
    join(root, "lib", "pkgconfig"),
    join(root, "share", "pkgconfig"),
    environmentValue(environment, "PKG_CONFIG_PATH"),
  ].filter(Boolean).join(delimiter));
  // Node can inherit both Path and PATH on Windows. Normalize them so CreateProcess does not
  // choose an environment entry that accidentally hides cargo.exe.
  setWindowsEnvironmentValue(environment, "Path", [bin, ...cleanPath].join(delimiter));
  console.log(`[HeriHeriCloud] Using GStreamer for Windows ${architecture} from ${root}`);
}

function configureLinuxGStreamer(environment) {
  if (!commandSucceeds("pkg-config", ["--exists", "gstreamer-1.0", "gstreamer-video-1.0"]) ||
      !commandSucceeds("gst-inspect-1.0", ["playbin"]) || !commandSucceeds("gst-inspect-1.0", ["avdec_h265"])) {
    console.log("[HeriHeriCloud] The Linux GStreamer development SDK or HEVC decoder is missing; installing distribution packages.");
    runSetup("linux");
  }
  if (!commandSucceeds("pkg-config", ["--exists", "gstreamer-1.0", "gstreamer-video-1.0"]) ||
      !commandSucceeds("gst-inspect-1.0", ["playbin"]) || !commandSucceeds("gst-inspect-1.0", ["avdec_h265"])) {
    throw new Error("Linux GStreamer setup finished without the development SDK, playbin, or avdec_h265. Your distribution may require an additional multimedia repository for gst-libav.");
  }
  environment.PKG_CONFIG_PATH = environmentValue(environment, "PKG_CONFIG_PATH") || "";
  console.log("[HeriHeriCloud] Using the system GStreamer installation");
}

function configureMacOSGStreamer(environment) {
  const framework = "/Library/Frameworks/GStreamer.framework/Versions/1.0";
  const pkgConfig = join(framework, "lib", "pkgconfig");
  if (!existsSync(join(pkgConfig, "gstreamer-1.0.pc")) &&
      !commandSucceeds("pkg-config", ["--exists", "gstreamer-1.0", "gstreamer-video-1.0"])) {
    console.log("[HeriHeriCloud] GStreamer for macOS was not found; fetching the latest stable runtime and development packages.");
    runSetup("macos");
  }
  if (existsSync(join(pkgConfig, "gstreamer-1.0.pc"))) {
    environment.PKG_CONFIG_PATH = [pkgConfig, environmentValue(environment, "PKG_CONFIG_PATH")].filter(Boolean).join(delimiter);
    environment.PATH = [join(framework, "bin"), environmentValue(environment, "PATH")].filter(Boolean).join(delimiter);
  }
  console.log("[HeriHeriCloud] GStreamer for macOS is ready");
}

function validAndroidRoot(root) {
  const marker = join("share", "gst-android", "ndk-build", "gstreamer-1.0.mk");
  return Boolean(root) && ["arm64", "armv7", "x86", "x86_64"].every((architecture) =>
    existsSync(join(root, architecture, marker))
  );
}

function normalizeAndroidRoot(root) {
  if (validAndroidRoot(root)) return root;
  const parent = root ? resolve(root, "..") : "";
  return validAndroidRoot(parent) ? parent : root;
}

function configureAndroidNdk(environment) {
  const sdkRoot = environmentValue(environment, "ANDROID_HOME") || environmentValue(environment, "ANDROID_SDK_ROOT");
  const configuredRoot = environmentValue(environment, "NDK_HOME");
  const expectedRoot = sdkRoot ? join(sdkRoot, "ndk", androidNdkVersion) : configuredRoot;
  if (!expectedRoot || !existsSync(join(expectedRoot, "source.properties"))) {
    throw new Error(
      `Android NDK ${androidNdkVersion} was not found${expectedRoot ? ` at ${expectedRoot}` : " because ANDROID_HOME/ANDROID_SDK_ROOT is unset"}. ` +
      `Install 'ndk;${androidNdkVersion}' in the Android SDK Manager, then retry.`,
    );
  }

  const setValue = process.platform === "win32" ? setWindowsEnvironmentValue : (target, name, value) => { target[name] = value; };
  setValue(environment, "NDK_HOME", expectedRoot);
  setValue(environment, "ANDROID_NDK_HOME", expectedRoot);
  setValue(environment, "ANDROID_NDK_ROOT", expectedRoot);
  setValue(environment, "HERI_ANDROID_NDK_VERSION", androidNdkVersion);
  console.log(`[HeriHeriCloud] Using unified Android NDK ${androidNdkVersion} from ${expectedRoot}`);
}

function configureAndroidGStreamer(environment) {
  let root = normalizeAndroidRoot(environmentValue(environment, "GSTREAMER_ROOT_ANDROID"));
  const pointer = resolve(projectRoot, ".gstreamer", "android", "current-root.txt");
  if (!validAndroidRoot(root) && existsSync(pointer)) root = normalizeAndroidRoot(readFileSync(pointer, "utf8").trim());
  if (!validAndroidRoot(root)) {
    console.log("[HeriHeriCloud] Android GStreamer SDK was not found; fetching the latest stable universal SDK (large download).");
    runSetup("android");
    if (existsSync(pointer)) root = readFileSync(pointer, "utf8").trim();
  }
  if (!validAndroidRoot(root)) throw new Error("Android GStreamer setup completed, but GSTREAMER_ROOT_ANDROID is invalid");
  writeFileSync(pointer, root, "utf8");
  environment.GSTREAMER_ROOT_ANDROID = root;
  environment.HERI_GSTREAMER_ANDROID_ROOT = root;
  console.log(`[HeriHeriCloud] Using Android GStreamer from ${root}`);
}

function configureIOSGStreamer() {
  const framework = resolve(projectRoot, "plugins", "tauri-plugin-gstreamer-player", "ios", "Frameworks", "GStreamer.xcframework", "Info.plist");
  if (!existsSync(framework)) {
    console.log("[HeriHeriCloud] iOS GStreamer XCFramework was not found; fetching the latest stable XCFramework.");
    runSetup("ios");
  }
  if (!existsSync(framework)) throw new Error("iOS GStreamer setup completed, but GStreamer.xcframework is missing");
}

const environment = { ...process.env };
if (isAndroidTarget) {
  configureAndroidNdk(environment);
  configureAndroidGStreamer(environment);
}
else if (isIOSTarget) configureIOSGStreamer();
else if (process.platform === "win32") configureWindowsGStreamer(environment);
else if (process.platform === "darwin") configureMacOSGStreamer(environment);
else if (process.platform === "linux") configureLinuxGStreamer(environment);

const result = spawnSync(process.execPath, [tauriCli, ...args], {
  cwd: projectRoot,
  env: environment,
  stdio: "inherit",
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
