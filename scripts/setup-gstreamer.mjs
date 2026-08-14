import { createHash } from "node:crypto";
import { createReadStream, createWriteStream, existsSync } from "node:fs";
import { mkdir, readFile, readdir, rename, rm, writeFile } from "node:fs/promises";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cacheRoot = resolve(projectRoot, ".gstreamer");
const pluginRoot = resolve(projectRoot, "plugins", "tauri-plugin-gstreamer-player");
const argumentsMap = new Map();
for (let index = 2; index < process.argv.length; index += 1) {
  const argument = process.argv[index];
  if (argument.startsWith("--")) argumentsMap.set(argument.slice(2), process.argv[index + 1]?.startsWith("--") ? true : process.argv[++index]);
}

function requestedPlatform() {
  const configured = String(argumentsMap.get("platform") || "").toLowerCase();
  if (configured) return configured;
  if (process.platform === "win32") return "windows";
  if (process.platform === "darwin") return "macos";
  if (process.platform === "linux") return "linux";
  throw new Error(`Automatic GStreamer setup is not supported on host '${process.platform}'`);
}

function requestedWindowsArchitecture() {
  const configured = String(argumentsMap.get("arch") || process.arch).toLowerCase();
  if (/arm64|aarch64/.test(configured)) return "arm64";
  if (/ia32|i686|x86$/.test(configured)) return "x86";
  return "x86_64";
}

function compareVersions(left, right) {
  const a = left.split(".").map(Number);
  const b = right.split(".").map(Number);
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const difference = (a[index] || 0) - (b[index] || 0);
    if (difference) return difference;
  }
  return 0;
}

async function fetchText(url) {
  const response = await fetch(url, { redirect: "follow", signal: AbortSignal.timeout(20_000) });
  if (!response.ok) throw new Error(`Unable to fetch ${url}: HTTP ${response.status}`);
  return response.text();
}

async function latestStableVersion(releasePlatform) {
  const override = String(argumentsMap.get("version") || process.env.GSTREAMER_VERSION || "");
  if (override) return override;
  const index = await fetchText(`https://gstreamer.freedesktop.org/data/pkg/${releasePlatform}/`);
  const versions = [...index.matchAll(/href=["'](?:\.\/)?(\d+\.\d+\.\d+)\/["']/gi)]
    .map((match) => match[1])
    // GStreamer uses even minor versions for stable release series; odd minors are development.
    .filter((version) => Number(version.split(".")[1]) % 2 === 0)
    .sort(compareVersions);
  const latest = versions.at(-1);
  if (!latest) throw new Error(`Unable to determine the latest stable GStreamer ${releasePlatform} release`);
  return latest;
}

async function sha256(path) {
  const hash = createHash("sha256");
  await pipeline(createReadStream(path), hash);
  return hash.digest("hex");
}

async function downloadVerified(url, destination) {
  const expected = (await fetchText(`${url}.sha256sum`)).trim().split(/\s+/)[0].toLowerCase();
  if (existsSync(destination) && await sha256(destination) === expected) {
    console.log(`[HeriHeriCloud] Reusing verified ${destination}`);
    return;
  }

  await mkdir(dirname(destination), { recursive: true });
  const partial = `${destination}.partial`;
  await rm(partial, { force: true });
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok || !response.body) throw new Error(`Unable to download ${url}: HTTP ${response.status}`);
  const total = Number(response.headers.get("content-length")) || 0;
  let received = 0;
  let lastReported = 0;
  const progress = new Transform({
    transform(chunk, _encoding, callback) {
      received += chunk.length;
      if (received - lastReported >= 64 * 1024 * 1024) {
        lastReported = received;
        const detail = total ? `${Math.round(received * 100 / total)}%` : `${Math.round(received / 1024 / 1024)} MiB`;
        console.log(`[HeriHeriCloud] Downloading ${detail}`);
      }
      callback(null, chunk);
    },
  });
  console.log(`[HeriHeriCloud] Fetching ${url}`);
  await pipeline(Readable.fromWeb(response.body), progress, createWriteStream(partial));
  const actual = await sha256(partial);
  if (actual !== expected) {
    await rm(partial, { force: true });
    throw new Error(`GStreamer checksum mismatch: expected ${expected}, received ${actual}`);
  }
  await rm(destination, { force: true });
  await rename(partial, destination);
}

function run(command, args, description) {
  console.log(`[HeriHeriCloud] ${description}`);
  const result = spawnSync(command, args, { cwd: projectRoot, env: process.env, stdio: "inherit" });
  if (result.error) throw new Error(`${description} failed: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${description} exited with code ${result.status}`);
}

function commandSucceeds(command, args) {
  const result = spawnSync(command, args, { cwd: projectRoot, env: process.env, stdio: "ignore" });
  return !result.error && result.status === 0;
}

async function findDirectoryContaining(start, relativeMarker, maxDepth = 5) {
  const queue = [{ path: start, depth: 0 }];
  while (queue.length) {
    const current = queue.shift();
    if (existsSync(join(current.path, relativeMarker))) return current.path;
    if (current.depth >= maxDepth) continue;
    for (const entry of await readdir(current.path, { withFileTypes: true })) {
      if (entry.isDirectory()) queue.push({ path: join(current.path, entry.name), depth: current.depth + 1 });
    }
  }
  return null;
}

async function setupWindows() {
  if (process.platform !== "win32") throw new Error("The Windows GStreamer installer must run on Windows");
  const version = await latestStableVersion("windows");
  const architecture = requestedWindowsArchitecture();
  run(
    "powershell.exe",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", resolve(projectRoot, "scripts", "setup-gstreamer-windows.ps1"), "-Version", version, "-Architecture", architecture],
    `Installing GStreamer ${version} for Windows ${architecture}`,
  );
}

function privileged(command, args, description) {
  if (typeof process.getuid === "function" && process.getuid() === 0) run(command, args, description);
  else run("sudo", [command, ...args], description);
}

async function setupLinux() {
  if (process.platform !== "linux") throw new Error("Linux GStreamer packages must be installed on Linux");
  if (commandSucceeds("pkg-config", ["--exists", "gstreamer-1.0", "gstreamer-video-1.0"]) &&
      commandSucceeds("gst-inspect-1.0", ["playbin"]) && commandSucceeds("gst-inspect-1.0", ["avdec_h265"])) {
    console.log("[HeriHeriCloud] The system GStreamer development SDK and HEVC decoder are ready");
    return;
  }
  if (commandSucceeds("sh", ["-c", "command -v apt-get"])) {
    privileged("apt-get", ["update"], "Refreshing Debian/Ubuntu package metadata");
    privileged("apt-get", ["install", "-y", "pkg-config", "libgstreamer1.0-dev", "libgstreamer-plugins-base1.0-dev", "libgstreamer-plugins-bad1.0-dev", "gstreamer1.0-tools", "gstreamer1.0-plugins-base", "gstreamer1.0-plugins-good", "gstreamer1.0-plugins-bad", "gstreamer1.0-plugins-ugly", "gstreamer1.0-libav", "gstreamer1.0-gl", "gstreamer1.0-gtk3", "gstreamer1.0-pulseaudio"], "Installing GStreamer from Debian/Ubuntu repositories");
  } else if (commandSucceeds("sh", ["-c", "command -v dnf"])) {
    privileged("dnf", ["install", "-y", "pkgconf-pkg-config", "gstreamer1-devel", "gstreamer1-plugins-base-devel", "gstreamer1-plugins-base-tools", "gstreamer1-plugins-good", "gstreamer1-plugins-bad-free", "gstreamer1-plugins-bad-free-devel", "gstreamer1-plugins-ugly", "gstreamer1-libav"], "Installing GStreamer from Fedora repositories");
  } else if (commandSucceeds("sh", ["-c", "command -v pacman"])) {
    privileged("pacman", ["-S", "--needed", "--noconfirm", "pkgconf", "gstreamer", "gst-plugins-base", "gst-plugins-good", "gst-plugins-bad", "gst-plugins-ugly", "gst-libav"], "Installing GStreamer from Arch repositories");
  } else {
    throw new Error("No supported Linux package manager was found. Install GStreamer development, base/good/bad/ugly, and libav packages, then retry.");
  }
  if (!commandSucceeds("pkg-config", ["--exists", "gstreamer-1.0", "gstreamer-video-1.0"]) ||
      !commandSucceeds("gst-inspect-1.0", ["playbin"]) || !commandSucceeds("gst-inspect-1.0", ["avdec_h265"])) {
    throw new Error("GStreamer installed without the development SDK, playbin, or avdec_h265. Enable your distribution's multimedia repository for gst-libav, then retry.");
  }
}

async function setupMacOS() {
  if (process.platform !== "darwin") throw new Error("The macOS GStreamer packages must be installed on macOS");
  const version = await latestStableVersion("osx");
  const baseUrl = `https://gstreamer.freedesktop.org/data/pkg/osx/${version}`;
  const cache = resolve(cacheRoot, "cache");
  const packages = [
    `gstreamer-1.0-${version}-universal.pkg`,
    `gstreamer-1.0-devel-${version}-universal.pkg`,
  ];
  for (const name of packages) {
    const path = resolve(cache, name);
    await downloadVerified(`${baseUrl}/${name}`, path);
    privileged("installer", ["-pkg", path, "-target", "/"], `Installing ${name}`);
  }
}

async function setupAndroid() {
  const configured = process.env.GSTREAMER_ROOT_ANDROID;
  const marker = join("share", "gst-android", "ndk-build", "gstreamer-1.0.mk");
  if (configured && existsSync(join(configured, marker))) {
    console.log(`[HeriHeriCloud] Using configured Android GStreamer SDK at ${configured}`);
    return;
  }
  const version = await latestStableVersion("android");
  const archiveName = `gstreamer-1.0-android-universal-${version}.tar.xz`;
  const archive = resolve(cacheRoot, "cache", archiveName);
  await downloadVerified(`https://gstreamer.freedesktop.org/data/pkg/android/${version}/${archiveName}`, archive);
  const extraction = resolve(cacheRoot, "android", version);
  await rm(extraction, { recursive: true, force: true });
  await mkdir(extraction, { recursive: true });
  run("tar", ["-xf", archive, "-C", extraction], `Extracting Android GStreamer ${version}`);
  const root = await findDirectoryContaining(extraction, marker);
  if (!root) throw new Error("The Android archive did not contain the expected gst-android ndk-build SDK");
  await writeFile(resolve(cacheRoot, "android", "current-root.txt"), root, "utf8");
  console.log(`[HeriHeriCloud] Android GStreamer ${version} is ready at ${root}`);
}

async function setupIOS() {
  if (process.platform !== "darwin") throw new Error("iOS GStreamer setup must run on the macOS host used for the iOS build");
  const version = await latestStableVersion("ios");
  const destination = resolve(pluginRoot, "ios", "Frameworks", "GStreamer.xcframework");
  const versionMarker = resolve(pluginRoot, "ios", "Frameworks", ".gstreamer-version");
  const installedVersion = existsSync(versionMarker) ? (await readFile(versionMarker, "utf8")).trim() : "";
  if (installedVersion === version && existsSync(resolve(destination, "Info.plist"))) {
    console.log(`[HeriHeriCloud] iOS GStreamer ${version} is already ready`);
    return;
  }
  const archiveName = `gstreamer-${version}-xcframework.tar.xz`;
  const archive = resolve(cacheRoot, "cache", archiveName);
  await downloadVerified(`https://gstreamer.freedesktop.org/data/pkg/ios/${version}/${archiveName}`, archive);
  const extraction = resolve(cacheRoot, "ios", `staging-${version}`);
  await rm(extraction, { recursive: true, force: true });
  await mkdir(extraction, { recursive: true });
  run("tar", ["-xf", archive, "-C", extraction], `Extracting iOS GStreamer ${version}`);
  const frameworkParent = await findDirectoryContaining(extraction, join("GStreamer.xcframework", "Info.plist"));
  if (!frameworkParent) throw new Error("The iOS archive did not contain GStreamer.xcframework");
  await rm(destination, { recursive: true, force: true });
  await rename(resolve(frameworkParent, "GStreamer.xcframework"), destination);
  await writeFile(versionMarker, `${version}\n`, "utf8");
  await rm(extraction, { recursive: true, force: true });
  console.log(`[HeriHeriCloud] iOS GStreamer ${version} is ready at ${destination}`);
}

const platform = requestedPlatform();
if (argumentsMap.has("resolve-only")) {
  const releasePlatform = platform === "windows" ? "windows" : platform === "macos" || platform === "darwin" ? "osx" : platform;
  if (releasePlatform === "linux") console.log("Linux uses the newest GStreamer packages supplied by the active distribution repositories.");
  else console.log(await latestStableVersion(releasePlatform));
} else {
  if (platform === "windows") await setupWindows();
  else if (platform === "linux") await setupLinux();
  else if (platform === "macos" || platform === "darwin") await setupMacOS();
  else if (platform === "android") await setupAndroid();
  else if (platform === "ios") await setupIOS();
  else throw new Error(`Unknown GStreamer setup platform '${platform}'`);
}
