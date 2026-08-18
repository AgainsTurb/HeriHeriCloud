import { cp, mkdir, readdir, rm, stat, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { basename, dirname, join, normalize, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const COMMON_PLUGINS = [
  "coreelements", "playback", "typefindfunctions", "autodetect",
  "audioconvert", "audioresample", "volume", "videoconvertscale",
  "soup", "isomp4", "matroska", "avi", "asf", "mpegtsdemux",
  "mpegpsdemux", "ogg", "audioparsers", "videoparsersbad", "libav",
  "subparse", "pango", "flac", "wavparse", "mpg123", "opus", "vpx",
];

const WINDOWS_PLUGINS = ["d3d11", "wasapi2"];
const MACOS_PLUGINS = ["osxaudio", "osxvideo", "applemedia"];
const LINUX_VIDEO_PLUGINS = ["ximagesink", "wayland", "gtk", "opengl"];
const LINUX_AUDIO_PLUGINS = ["pulseaudio", "alsa"];
const WINDOWS_LIMIT = 220 * 1024 * 1024;
const MACOS_LIMIT = 300 * 1024 * 1024;
const LINUX_PLUGIN_LIMIT = 100 * 1024 * 1024;
const RELEASE_ARTIFACT_LIMIT = 300 * 1024 * 1024;

function run(command, args, description, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
    stdio: options.capture ? "pipe" : "inherit",
  });
  if (result.error) throw new Error(`${description} failed: ${result.error.message}`);
  if (result.status !== 0) {
    const detail = options.capture ? `\n${result.stdout || ""}${result.stderr || ""}`.trimEnd() : "";
    throw new Error(`${description} exited with code ${result.status}${detail}`);
  }
  return result.stdout || "";
}

async function directoryFiles(path, suffix) {
  return (await readdir(path, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.toLowerCase().endsWith(suffix))
    .map((entry) => join(path, entry.name));
}

async function totalSize(root) {
  let size = 0;
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    size += entry.isDirectory() ? await totalSize(path) : (await stat(path)).size;
  }
  return size;
}

function pluginPath(pluginDirectory, name, prefix, extension) {
  return join(pluginDirectory, `${prefix}gst${name}${extension}`);
}

function windowsDumpbin() {
  const direct = spawnSync("where.exe", ["dumpbin.exe"], { encoding: "utf8" });
  const directPath = direct.status === 0 ? direct.stdout.trim().split(/\r?\n/)[0] : "";
  if (directPath && existsSync(directPath)) return directPath;

  const vswhere = join(
    process.env["ProgramFiles(x86)"] || "C:\\Program Files (x86)",
    "Microsoft Visual Studio", "Installer", "vswhere.exe",
  );
  if (existsSync(vswhere)) {
    const found = spawnSync(vswhere, [
      "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
      "-find", "VC\\Tools\\MSVC\\**\\bin\\Hostx64\\x64\\dumpbin.exe",
    ], { encoding: "utf8" });
    const path = found.status === 0 ? found.stdout.trim().split(/\r?\n/).at(-1) : "";
    if (path && existsSync(path)) return path;
  }
  throw new Error("dumpbin.exe is required to select the Windows GStreamer runtime dependency closure");
}

function windowsDependencies(dumpbin, path) {
  const output = run(dumpbin, ["/nologo", "/dependents", path], `Inspecting ${basename(path)}`, { capture: true });
  return [...output.matchAll(/^\s+([\w.+-]+\.dll)\s*$/gim)].map((match) => match[1]);
}

async function prepareWindows(projectRoot, sdkRoot, architecture) {
  const sourceBin = join(sdkRoot, "bin");
  const sourcePlugins = join(sdkRoot, "lib", "gstreamer-1.0");
  const sourceScanner = join(sdkRoot, "libexec", "gstreamer-1.0", "gst-plugin-scanner.exe");
  const stage = resolve(projectRoot, ".gstreamer", "bundle", `windows-${architecture}`);
  const stagePlugins = join(stage, "gstreamer-1.0");
  await rm(stage, { recursive: true, force: true });
  await mkdir(stagePlugins, { recursive: true });

  const plugins = [...COMMON_PLUGINS, ...WINDOWS_PLUGINS]
    .map((name) => pluginPath(sourcePlugins, name, "", ".dll"));
  const missing = plugins.filter((path) => !existsSync(path));
  if (missing.length) throw new Error(`The Windows GStreamer SDK is missing required plugins:\n${missing.join("\n")}`);
  if (!existsSync(sourceScanner)) throw new Error(`The Windows GStreamer plugin scanner is missing: ${sourceScanner}`);

  const available = new Map((await directoryFiles(sourceBin, ".dll")).map((path) => [basename(path).toLowerCase(), path]));
  const dumpbin = windowsDumpbin();
  const queue = [
    ...plugins,
    sourceScanner,
    ...["gstreamer-1.0-0.dll", "gstvideo-1.0-0.dll"].map((name) => available.get(name)),
  ].filter(Boolean);
  const runtime = new Map();
  for (let index = 0; index < queue.length; index += 1) {
    const path = queue[index];
    const key = normalize(path).toLowerCase();
    if (runtime.has(key)) continue;
    runtime.set(key, path);
    for (const dependency of windowsDependencies(dumpbin, path)) {
      const sdkDependency = available.get(dependency.toLowerCase());
      if (sdkDependency) queue.push(sdkDependency);
    }
  }

  for (const path of runtime.values()) {
    const destination = plugins.includes(path)
      ? join(stagePlugins, basename(path))
      : join(stage, basename(path));
    await cp(path, destination);
  }
  const licenses = join(sdkRoot, "share", "licenses");
  if (existsSync(licenses)) await cp(licenses, join(stage, "gstreamer-licenses"), { recursive: true });
  const size = await totalSize(stage);
  if (size > WINDOWS_LIMIT) throw new Error(`Selected Windows GStreamer runtime is unexpectedly large (${Math.ceil(size / 1024 / 1024)} MiB)`);
  console.log(`[HeriHeriCloud] Bundling ${runtime.size} Windows GStreamer runtime files (${Math.ceil(size / 1024 / 1024)} MiB uncompressed)`);
  return { bundle: { resources: { [stage]: "" } } };
}

function macDependencies(path) {
  const output = run("otool", ["-L", path], `Inspecting ${basename(path)}`, { capture: true });
  return output.split(/\r?\n/).slice(1).map((line) => line.trim().split(/\s+\(/)[0]).filter(Boolean);
}

function resolveMacDependency(sourceRoot, sourceFile, dependency) {
  if (dependency.startsWith(`${sourceRoot}/`)) return dependency;
  if (dependency.startsWith("@loader_path/")) return resolve(dirname(sourceFile), dependency.slice(13));
  if (dependency.startsWith("@rpath/")) return join(sourceRoot, "lib", basename(dependency));
  const frameworkPrefix = "/Library/Frameworks/GStreamer.framework/Versions/1.0/";
  if (dependency.startsWith(frameworkPrefix)) return join(sourceRoot, dependency.slice(frameworkPrefix.length));
  return null;
}

async function prepareMacOS(projectRoot, sourceRoot) {
  const sourcePlugins = join(sourceRoot, "lib", "gstreamer-1.0");
  const sourceScanner = join(sourceRoot, "libexec", "gstreamer-1.0", "gst-plugin-scanner");
  const stage = resolve(projectRoot, ".gstreamer", "bundle", "macos-universal");
  const frameworks = join(stage, "Frameworks");
  const pluginsDirectory = join(stage, "Resources", "gstreamer-1.0");
  const helpers = join(stage, "Helpers");
  const licensesDirectory = join(stage, "Resources", "gstreamer-licenses");
  await rm(stage, { recursive: true, force: true });
  await Promise.all([mkdir(frameworks, { recursive: true }), mkdir(pluginsDirectory, { recursive: true }), mkdir(helpers, { recursive: true })]);

  const common = COMMON_PLUGINS.map((name) => pluginPath(sourcePlugins, name, "lib", ".dylib"));
  const platformCandidates = MACOS_PLUGINS.map((name) => pluginPath(sourcePlugins, name, "lib", ".dylib"));
  const platformPlugins = platformCandidates.filter(existsSync);
  const missing = common.filter((path) => !existsSync(path));
  if (missing.length) throw new Error(`The macOS GStreamer SDK is missing required plugins:\n${missing.join("\n")}`);
  if (!existsSync(platformCandidates[0]) || !platformCandidates.slice(1).some(existsSync)) {
    throw new Error("The macOS GStreamer SDK requires osxaudio plus an osxvideo/applemedia video sink plugin");
  }
  if (!existsSync(sourceScanner)) throw new Error(`The macOS GStreamer plugin scanner is missing: ${sourceScanner}`);

  const seeds = [
    ...common.map((source) => ({ source, destination: join(pluginsDirectory, basename(source)), kind: "plugin" })),
    ...platformPlugins.map((source) => ({ source, destination: join(pluginsDirectory, basename(source)), kind: "plugin" })),
    { source: sourceScanner, destination: join(helpers, "gst-plugin-scanner"), kind: "scanner" },
    ...["libgstreamer-1.0.0.dylib", "libgstvideo-1.0.0.dylib"].map((name) => ({
      source: join(sourceRoot, "lib", name), destination: join(frameworks, name), kind: "library",
    })),
  ];
  const copied = new Map();
  const dependencyChanges = new Map();
  for (let index = 0; index < seeds.length; index += 1) {
    const item = seeds[index];
    const key = normalize(item.source);
    if (copied.has(key)) continue;
    if (!existsSync(item.source)) throw new Error(`Required macOS GStreamer runtime file is missing: ${item.source}`);
    await cp(item.source, item.destination);
    copied.set(key, item);
    const changes = [];
    for (const dependency of macDependencies(item.source)) {
      const resolved = resolveMacDependency(sourceRoot, item.source, dependency);
      if (!resolved || !existsSync(resolved) || resolved === item.source) continue;
      const bundledDependency = item.kind === "library"
        ? `@loader_path/${basename(resolved)}`
        : item.kind === "plugin"
          ? `@loader_path/../../Frameworks/${basename(resolved)}`
          : `@loader_path/../Frameworks/${basename(resolved)}`;
      changes.push([dependency, bundledDependency]);
      seeds.push({ source: resolved, destination: join(frameworks, basename(resolved)), kind: "library" });
    }
    dependencyChanges.set(key, changes);
  }

  for (const [source, item] of copied) {
    const changes = dependencyChanges.get(source) || [];
    for (const [oldName, newName] of changes) {
      run("install_name_tool", ["-change", oldName, newName, item.destination], `Rewriting ${basename(item.destination)}`);
    }
    if (item.kind === "library") {
      run("install_name_tool", ["-id", `@rpath/${basename(item.destination)}`, item.destination], `Setting ${basename(item.destination)} install name`);
    }
  }

  const licenses = join(sourceRoot, "share", "licenses");
  if (existsSync(licenses)) await cp(licenses, licensesDirectory, { recursive: true });

  const size = await totalSize(stage);
  if (size > MACOS_LIMIT) throw new Error(`Selected macOS GStreamer runtime is unexpectedly large (${Math.ceil(size / 1024 / 1024)} MiB)`);
  console.log(`[HeriHeriCloud] Bundling ${copied.size} macOS GStreamer runtime files (${Math.ceil(size / 1024 / 1024)} MiB uncompressed)`);
  const files = {};
  for (const item of copied.values()) {
    const relative = item.destination.slice(stage.length + 1).replaceAll("\\", "/");
    files[relative] = item.destination;
  }
  if (existsSync(licensesDirectory)) files["Resources/gstreamer-licenses"] = licensesDirectory;
  return { bundle: { macOS: { files } } };
}

function linuxBundleConfig() {
  return {
    bundle: {
      linux: {
        appimage: { bundleMediaFramework: true },
        deb: {
          depends: [
            "gstreamer1.0-plugins-base", "gstreamer1.0-plugins-good",
            "gstreamer1.0-plugins-bad", "gstreamer1.0-plugins-ugly", "gstreamer1.0-libav",
            "gstreamer1.0-gl", "gstreamer1.0-pulseaudio",
          ],
        },
      },
    },
  };
}

function commandOutput(command, args, description) {
  return run(command, args, description, { capture: true }).trim();
}

async function prepareLinux(projectRoot, environment) {
  const sourcePlugins = commandOutput(
    "pkg-config", ["--variable=pluginsdir", "gstreamer-1.0"], "Locating Linux GStreamer plugins",
  );
  const pluginScannerDir = commandOutput(
    "pkg-config", ["--variable=pluginscannerdir", "gstreamer-1.0"], "Locating Linux GStreamer plugin scanner",
  );
  const libdir = commandOutput(
    "pkg-config", ["--variable=libdir", "gstreamer-1.0"], "Locating Linux GStreamer libraries",
  );
  const libexec = commandOutput(
    "pkg-config", ["--variable=libexecdir", "gstreamer-1.0"], "Locating Linux GStreamer helpers",
  );
  const helperCandidates = [
    pluginScannerDir,
    join(libdir, "gstreamer1.0", "gstreamer-1.0"),
    join(libexec, "gstreamer-1.0"),
    libexec,
  ].filter(Boolean);
  const sourceHelpers = helperCandidates.find((path) => existsSync(join(path, "gst-plugin-scanner")));
  if (!sourceHelpers) {
    throw new Error(`The Linux GStreamer plugin scanner was not found in: ${helperCandidates.join(", ")}`);
  }

  const stage = resolve(projectRoot, ".gstreamer", "bundle", "linux-native");
  const stagePlugins = join(stage, "plugins");
  const stageHelpers = join(stage, "helpers");
  await rm(stage, { recursive: true, force: true });
  await Promise.all([mkdir(stagePlugins, { recursive: true }), mkdir(stageHelpers, { recursive: true })]);

  const combinedVideoConvert = pluginPath(sourcePlugins, "videoconvertscale", "lib", ".so");
  const linuxCommonPlugins = existsSync(combinedVideoConvert)
    ? COMMON_PLUGINS
    : COMMON_PLUGINS.flatMap((name) => name === "videoconvertscale" ? ["videoconvert", "videoscale"] : [name]);
  const required = linuxCommonPlugins.map((name) => pluginPath(sourcePlugins, name, "lib", ".so"));
  const missing = required.filter((path) => !existsSync(path));
  if (missing.length) throw new Error(`The Linux GStreamer installation is missing required plugins:\n${missing.join("\n")}`);
  const video = LINUX_VIDEO_PLUGINS.map((name) => pluginPath(sourcePlugins, name, "lib", ".so")).filter(existsSync);
  const audio = LINUX_AUDIO_PLUGINS.map((name) => pluginPath(sourcePlugins, name, "lib", ".so")).filter(existsSync);
  if (!video.length) throw new Error("The Linux GStreamer installation has no supported X11/Wayland/GTK/GL video sink plugin");
  if (!audio.length) throw new Error("The Linux GStreamer installation has no supported PulseAudio/ALSA sink plugin");

  const plugins = [...required, ...video, ...audio];
  for (const path of plugins) await cp(path, join(stagePlugins, basename(path)));
  for (const name of ["gst-plugin-scanner", "gst-ptp-helper"]) {
    const source = join(sourceHelpers, name);
    if (existsSync(source)) await cp(source, join(stageHelpers, name));
  }
  const size = await totalSize(stage);
  if (size > LINUX_PLUGIN_LIMIT) throw new Error(`Selected Linux GStreamer plugins are unexpectedly large (${Math.ceil(size / 1024 / 1024)} MiB)`);
  environment.GSTREAMER_PLUGINS_DIR = stagePlugins;
  environment.GSTREAMER_HELPERS_DIR = stageHelpers;
  console.log(`[HeriHeriCloud] Bundling ${plugins.length} Linux GStreamer plugins (${Math.ceil(size / 1024 / 1024)} MiB before shared dependencies)`);
  return linuxBundleConfig();
}

async function releaseArtifacts(root) {
  if (!existsSync(root)) return [];
  const artifacts = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) artifacts.push(...await releaseArtifacts(path));
    else if (/\.(?:msi|exe|deb|rpm|appimage|dmg)$|\.app\.tar\.gz$/i.test(entry.name)) artifacts.push(path);
  }
  return artifacts;
}

export async function auditDesktopBundleSizes(projectRoot, targetRoot) {
  const bundleRoot = targetRoot || resolve(projectRoot, "src-tauri", "target");
  const artifacts = (await releaseArtifacts(bundleRoot)).filter((path) => /[\\/]bundle[\\/]/i.test(path));
  if (!artifacts.length) throw new Error(`No desktop release artifacts were found below ${bundleRoot}`);
  for (const path of artifacts) {
    const size = (await stat(path)).size;
    const sizeMiB = Math.ceil(size / 1024 / 1024);
    if (size > RELEASE_ARTIFACT_LIMIT) {
      throw new Error(`Release artifact exceeds the 300 MiB limit: ${path} (${sizeMiB} MiB)`);
    }
    console.log(`[HeriHeriCloud] Release artifact size: ${basename(path)} (${sizeMiB} MiB)`);
  }
}

export async function prepareGStreamerBundle({ projectRoot, platform, sdkRoot, architecture, environment = process.env }) {
  let config;
  if (platform === "windows") config = await prepareWindows(projectRoot, sdkRoot, architecture);
  else if (platform === "macos") config = await prepareMacOS(projectRoot, sdkRoot);
  else if (platform === "linux") config = await prepareLinux(projectRoot, environment);
  else return null;

  const configDirectory = resolve(projectRoot, ".gstreamer", "bundle", `config-${platform}-${architecture || "native"}`);
  await mkdir(configDirectory, { recursive: true });
  const configPath = join(configDirectory, "tauri.bundle.conf.json");
  await writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
  return configPath;
}
