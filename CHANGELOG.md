# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
---

## [1.1.2] – 2026-07-22

### 新增
- 无。

### 变更
- 增强文件续传的功能。

### 修复
- 修复主页无法搜索文件的问题。

### Added
- None.

### Changed
- Enhanced file resume capability.

### Fixed
- Fixed an issue where files could not be searched for on the home page.

---


---

## [1.1.1] – 2026-07-15

### 新增
- 无。

### 变更
- 将vfs升级成v2版本，使用base64加密文件名，防止“|”符号冲突，同时会将所有仍为v1版本的本地文件升级成v2版本，确保已经有的会被错误识别的文件名也能正确被识别已经升级。
- 使用cdn获取Github Release信息，避免获取最新软件版本失败的情况。

### 修复
- 无。

### Added
- None.

### Changed
- The VFS is being upgraded to version 2, utilizing Base64 encoding for filenames to prevent conflicts involving the | character. Additionally, all existing local files still on version 1 will be upgraded to version 2, ensuring that filenames previously prone to misidentification are correctly recognized as upgraded.
- Use a CDN to retrieve GitHub release information to avoid failures in fetching the latest software version.

### Fixed
- None.

---

---

## [1.1.0] – 2026-07-01

### 新增
- 对于分片上传的大文件采用文件名加密来规避检测。

### 变更
- 无。

### 修复
- 无。

### Added
- File name encryption is employed for large files uploaded in chunks to evade detection.

### Changed
- None.

### Fixed
- None.

---

---

## [1.0.2] – 2026-07-01

### 新增
- 无。

### 变更
- 变更大文件的分片名。

### 修复
- 无。

### Added
- None.

### Changed
- Change the chunk names for large files.

### Fixed
- None.

---


---

## [1.0.1] – 2026-06-30

### 新增
- 无。

### 变更
- 无。

### 修复
- 修复大文件上传可能出现的卡停问题。

### Added
- None.

### Changed
- None.

### Fixed
- Fixed an issue where large file uploads might stall.

---


---

## [1.0.0] – 2026-06-28

### 新增
- 适配Windows，macOS和Linux，基本适配Android和iOS。
- 添加多语言支持（中文/英文）。
- 添加系统图标。
- 支持蓝奏云盘官方账户注册以及登录。
- 绕过蓝奏云盘100MB文件大小限制。
- 绕过蓝奏云盘文件扩展名限制。
- 绕过蓝奏云盘四级目录限制。
- 实现全部蓝奏云盘api。
- 实现文件管理器效果的云盘文件管理页面。
- 基本实现多设备同步。
- 支持文件分享以及文件租赁。
- 支持标准WebDAV挂载。
- 支持在线预览部分视频（视频编码原因），图片，文本以及PDF文档。
- 实现批量上传，批量下载，以及对应设置管理。
- 实现上传下载断点续传（上传受蓝奏云盘限制，只能每100MB续传）。

### 变更
- 无。

### 修复
- 无。

### Added
- Compatible with Windows, macOS, and Linux; basic support for Android and iOS.
- Added multi-language support (Chinese/English).
- Added system tray icons.
- Supports official account registration and login for Lanzou Cloud.
- Bypasses Lanzou Cloud's 100MB file size limit.
- Bypasses Lanzou Cloud's file extension restrictions.
- Bypasses Lanzou Cloud's four-level directory depth limit.
- Implemented the full Lanzou Cloud API.
- Implemented a cloud file management interface with a file-manager-like experience.
- Multi-device synchronization is basically implemented.
- Supports file sharing and file leasing.
- Supports standard WebDAV mounting.
- Supports online previewing of images, text, PDF documents, and selective videos (subject to video encoding compatibility).
- Implemented batch upload and download functions, along with corresponding management settings.
- Implemented resume-from-breakpoint for uploads and downloads (upload resumption is limited by Lanzou Cloud to 100MB segments).

### Changed
- None.

### Fixed
- None.

---