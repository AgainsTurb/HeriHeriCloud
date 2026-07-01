# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
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