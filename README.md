
[English](#english-version) | [中文](#chinese-version)
---
<a name="chinese-version"></a>
# HeriHeriCloud

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0) [![Tauri](https://img.shields.io/badge/Tauri-v2-FFC131?logo=tauri&logoColor=white)](https://tauri.app/) [![React](https://img.shields.io/badge/React-TypeScript-61DAFB?logo=react&logoColor=black)](https://react.dev/) [![Platform](https://img.shields.io/badge/Platform-Windows_|_macOS_|_Linux_|_Android_|_iOS_-blue?logo=github)](#)

HeriHeriCloud 是一个跨平台的云盘客户端解决方案。本项目以蓝奏云的无限存储空间和不限速带宽为底层基础设施，旨在为用户提供现代化、高性能的文件管理与分享体验。

我们的核心目标是践行互联网的开源与共享精神。通过引入原生 WebDAV 挂载等高级特性，HeriHeriCloud 打破了传统云盘的封闭生态，允许用户将云端资源无缝接入本地文件系统或第三方媒体中心（如 Infuse、Jellyfin），让资源分享变得前所未有的简单。

目前，PC 端（Windows、macOS、Linux）已处于稳定状态，移动端（iOS、Android）正在积极开发中。

## 核心特性

* **跨平台架构**：基于 Tauri v2 + TypeScript + React 构建，确保在提供原生级性能和极低内存占用的同时，实现全平台代码复用。
* **原生 WebDAV 服务**：内置轻量级 WebDAV 代理，将深层目录结构和文件直接映射为标准 WebDAV 协议，支持第三方播放器串流与系统级挂载。
* **自定义虚拟文件系统 (VFS)**：突破底层云盘的目录层级和文件类型限制，在本地构建并维护完整的树状目录结构。
* **高级媒体与文档预览**：内置视频流媒体实时缓冲代理，并支持文本、代码、图片及 PDF 文档的原生窗口预览。
* **极速并发传输**：支持自定义并发数与速度限制，充分利用带宽上限，实现大文件的切片上传与断点续传。

## 构建与运行

### 环境准备
确保您的计算机已安装以下环境：
* Node.js (建议 v20+)
* Rust 工具链 (rustup, cargo)
* Tauri V2 开发依赖 (如 macOS 的 Xcode 命令行工具，Linux 的 webkit2gtk 等)

### 编译步骤

1. 克隆项目仓库：
```bash
git clone [https://github.com/yourusername/HeriHeriCloud.git](https://github.com/yourusername/HeriHeriCloud.git)
cd HeriHeriCloud
```

2. 安装前端依赖：
```bash
npm install
```

3. **关键配置**：出于安全和加解密机制的需求，编译器在构建时需要读取一个环境变量。您必须在项目根目录或全局创建 `.cargo/config.toml` 文件，并提供一个测试密钥：

创建或编辑 `.cargo/config.toml`，填入以下内容：
```toml
[env]
HERIHERI_SECRET_KEY = "dummy_secret_key"
```

4. 启动开发环境：
```bash
npm run tauri dev
```

5. 编译生产版本：
```bash
npm run tauri build
```

## 贡献指南

我们非常欢迎来自社区的贡献，无论是功能开发、Bug 修复还是界面优化。

**代码规范**
为了保持代码库的统一性和可维护性，所有提交的代码中，**变量命名、函数命名以及代码注释必须全部使用英文**。

**交流与反馈**
GitHub 的 Issues 和 Pull Requests 讨论区支持使用任何语言（中文或英文皆可）。

## API 声明

本项目中涉及蓝奏云的所有底层 API 交互逻辑、协议解析及逆向工程工作，均由作者从零开始独立研究并编写，**未参考、借用或依赖任何其他的第三方开源项目或现有实现**。所有接口逻辑均保持绝对最新，因此本项目不涉及对其他非基础库项目的致谢声明。

## 结语

真诚欢迎各位提交 Issue 报告问题或发起 PR 改进代码。我们鼓励大家分发、打包并传播这个应用程序，共同建立一个开放的资源共享社区。

如果这个项目对您有帮助，请考虑留下一个 Star。

---

<a name="english-version"></a>
# HeriHeriCloud

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0) [![Tauri](https://img.shields.io/badge/Tauri-v2-FFC131?logo=tauri&logoColor=white)](https://tauri.app/) [![React](https://img.shields.io/badge/React-TypeScript-61DAFB?logo=react&logoColor=black)](https://react.dev/) [![Platform](https://img.shields.io/badge/Platform-Windows_|_macOS_|_Linux_|_Android_|_iOS_-blue?logo=github)](#)

HeriHeriCloud is a cross-platform cloud storage client solution. Utilizing Lanzou Cloud's unlimited storage capacity and uncapped bandwidth as the underlying infrastructure, this project aims to provide users with a modern, high-performance file management and sharing experience.

Our core goal is to practice the open-source and sharing spirit of the internet. By introducing advanced features like native WebDAV mounting, HeriHeriCloud breaks the closed ecosystems of traditional cloud drives. It allows users to seamlessly integrate cloud resources into local file systems or third-party media centers (like Infuse or Jellyfin), making resource sharing easier than ever before.

Currently, the PC client (Windows, macOS, Linux) is stable, and the mobile client (iOS, Android) is under active development.

## Key Features

* **Cross-Platform Architecture**: Built with Tauri v2 + TypeScript + React, ensuring native-level performance and exceptionally low memory usage while achieving code reuse across all platforms.
* **Native WebDAV Service**: Features a built-in lightweight WebDAV proxy that maps deep directory structures and files into the standard WebDAV protocol, supporting third-party player streaming and system-level mounting.
* **Virtual File System (VFS)**: Overcomes the directory depth and file type restrictions of the underlying cloud drive by building and maintaining a complete, local tree-based directory structure.
* **Advanced Media & Document Preview**: Includes a real-time buffering proxy for video streaming, along with native window previewers for text, code, images, and PDF documents.
* **High-Speed Concurrent Transfers**: Supports custom concurrency limits and speed throttling, fully utilizing bandwidth capacity for large file chunk uploads and resumable downloads.

## Build and Run

### Prerequisites
Ensure your machine has the following installed:
* Node.js (v20+ recommended)
* Rust Toolchain (rustup, cargo)
* Tauri V2 system dependencies (e.g., Xcode Command Line Tools for macOS, webkit2gtk for Linux)

### Build Instructions

1. Clone the repository:
```bash
git clone [https://github.com/yourusername/HeriHeriCloud.git](https://github.com/yourusername/HeriHeriCloud.git)
cd HeriHeriCloud
```

2. Install frontend dependencies:
```bash
npm install
```

3. **Critical Configuration**: Due to security and encryption mechanisms, the compiler requires a specific environment variable during the build process. You must create a `.cargo/config.toml` file in the project root or your global cargo directory and provide a dummy key:

Create or edit `.cargo/config.toml` and add the following:
```toml
[env]
HERIHERI_SECRET_KEY = "dummy_secret_key"
```

4. Run the development environment:
```bash
npm run tauri dev
```

5. Build the production release:
```bash
npm run tauri build
```

## Contributing

We warmly welcome contributions from the community, whether it is feature development, bug fixing, or UI improvements.

**Code Standards**
To maintain consistency and maintainability across the codebase, **all variable names, function names, and code comments within submitted code must be entirely in English.**

**Communication**
Discussions in GitHub Issues and Pull Requests are welcome in any language.

## API Statement

All underlying API interaction logic, protocol parsing, and reverse engineering related to Lanzou Cloud within this project were researched and written from scratch independently by the author. **No third-party open-source projects or existing implementations were referenced, borrowed, or relied upon.** All interface logic is strictly up to date. Therefore, no acknowledgements to other non-fundamental library projects are necessary or made here.

## Conclusion

We sincerely welcome anyone to submit Issues to report bugs or open PRs to improve the code. We highly encourage the distribution, packaging, and spreading of this application to help build an open resource-sharing community.

If you find this project helpful, please consider leaving a Star.