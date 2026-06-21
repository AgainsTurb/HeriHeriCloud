const ALLOWED_EXTS = new Set([
  "doc", "docx", "zip", "rar", "apk", "txt", "exe", "7z", "e", "z", "ct", "ke", "cetrainer", "db", "tar", "pdf", "w3x",
  "epub", "mobi", "azw", "azw3", "osk", "osz", "xpa", "cpk", "lua", "jar", "dmg", "ppt", "pptx", "xls", "xlsx", "mp3",
  "ipa", "iso", "img", "gho", "ttf", "ttc", "txf", "dwg", "bat", "imazingapp", "dll", "crx", "xapk", "conf",
  "deb", "rp", "rpm", "rplib", "mobileconfig", "appimage", "lolgezi", "flac",
  "cad", "hwt", "accdb", "ce", "xmind", "enc", "bds", "bdi", "ssf", "it",
  "pkg", "cfg", "mp4", "avi", "png", "jpeg", "jpg", "gif", "webp", "brushset"
]);

export function getFileIcon(filename: string, isDir: boolean): string {
  if (isDir) return "https://up.woozooo.com/images/folder_open.gif";

  // Extract the extension safely
  const parts = filename.split('.');
  const ext = parts.length > 1 ? parts.pop()?.toLowerCase() || "" : "";

  // If the extension is explicitly supported by Lanzou, use its specific icon
  if (ALLOWED_EXTS.has(ext)) {
    return `https://up.woozooo.com/images/filetype/${ext}.gif`;
  }

  // Fallback to standard file icon
  return "https://up.woozooo.com/images/filetype/file.gif";
}