<p align="center">
  <img src="apps/desktop/public/app-icon.png" width="80" height="80" alt="ScreenTranslate 图标">
</p>
<h1 align="center">ScreenTranslate</h1>
<p align="center"><strong>框选文字，译文留在原处。</strong></p>
<p align="center">截图翻译 · 双栏文字翻译 · 快速翻译框</p>
<p align="center"><strong>简体中文</strong> | <a href="README.en.md">English</a></p>
<p align="center">
  <a href="https://github.com/reevebyte/ScreenTranslate/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/reevebyte/ScreenTranslate?style=flat-square&amp;color=303030&amp;label=Release"></a>
  <a href="https://github.com/reevebyte/ScreenTranslate/actions/workflows/v2-ci.yml"><img alt="构建与检查" src="https://github.com/reevebyte/ScreenTranslate/actions/workflows/v2-ci.yml/badge.svg?branch=main"></a>
  <a href="LICENSE"><img alt="MIT 许可证" src="https://img.shields.io/badge/License-MIT-737373?style=flat-square"></a>
</p>
<p align="center">
  <a href="https://github.com/reevebyte/ScreenTranslate/releases/latest"><strong>下载最新版本</strong></a> ·
  <a href="#快速开始">使用指南</a> ·
  <a href="https://github.com/reevebyte/ScreenTranslate/releases">更新记录</a> ·
  <a href="https://github.com/reevebyte/ScreenTranslate/issues/new">反馈问题</a>
</p>

| 浅色 · 服务中心 | 深色 · 设置与更新 |
| :---: | :---: |
| <img src="docs/screenshots/services.png" width="380" alt="浅色服务中心，可分别配置翻译和 OCR 服务"> | <img src="docs/screenshots/about.png" width="380" alt="深色设置页，提供自动更新与版本检查"> |

截图来自 Windows 原生程序客户区。

## 下载与安装

前往 **[最新版本](https://github.com/reevebyte/ScreenTranslate/releases/latest)**，按系统选择安装包：

| 系统 | 选择的文件 |
| --- | --- |
| **Windows 10 / 11 · x64** | **`*x64-setup.exe`**，按提示安装；需要系统 WebView2 |
| macOS 14+ · Apple Silicon | `*aarch64.dmg` |
| macOS 14+ · Intel | `*x64.dmg` |
| Linux · x64 | `.AppImage`；也提供 `.deb` 与 `.rpm` |

`.sig`、`latest.json`、`latest-v2.json` 和 `.app.tar.gz` 用于自动更新，无需手动下载。GitHub 的 `Source code` 是源码，不是安装包。

Windows 已进行原生交互验证；macOS 与 Linux 已完成构建，仍需在对应系统验证完整使用体验。

## 能做什么

- **截图即译**：框选屏幕文字，在原位置查看译文；支持原译对照、缩放、拖动和文字校对。
- **双栏翻译**：原文与译文并排阅读，随时编辑、重新翻译或复制。
- **快速翻译**：通过快捷键唤出小窗口，输入或粘贴文字即可翻译。
- **自选服务**：翻译与 OCR 分开配置，支持 OpenAI 兼容接口、专用翻译 API、本地及云端 OCR。
- **阅读与更新**：浅色 / 深色主题、字号调整、中英文界面，以及应用内更新。

## 快速开始

1. **打开软件**：首次启动会显示使用指南，点击“开始使用”进入快捷操作。
2. **框选翻译**：按 `Ctrl+Alt+Q`，拖出选区后松开，译文会显示在原位置。
3. **按需配置**：在“服务中心”分别选择翻译与 OCR 服务，点击“设为当前”启用；只浏览服务不会切换配置。

| 操作 | 默认快捷键 |
| --- | --- |
| 框选翻译 | `Ctrl+Alt+Q` |
| 收起 / 显示截图 | `Ctrl+Alt+W` |
| 快速文字翻译 | `Ctrl+Alt+Space` |

快捷键可在“快捷操作”修改；界面语言在“设置 → 通用”切换。更新入口位于“设置 → 关于”，默认启动时检查更新。

默认使用有道云端 OCR 与 Google 网页翻译，需要联网。可在服务中心切换本地 OCR；翻译是否联网取决于所选服务。免费网页接口可能限流或失效，API 服务需要自行配置凭据，额度与费用由服务商决定。

## 数据与隐私

当前截图、输入文字和译文默认仅保存在会话内存中，关闭程序后清除；设置保存在本机，API Key 使用系统凭据库。云端 OCR 会接收选区图片，翻译服务会接收识别或输入的文字；本地 OCR 不上传截图。

## 帮助与反馈

[报告问题 / 功能建议](https://github.com/reevebyte/ScreenTranslate/issues/new) · [版本记录](https://github.com/reevebyte/ScreenTranslate/releases) · [自定义 OCR 接口](docs/custom-ocr-api.md)

<details>
<summary><strong>开发与构建</strong></summary>

使用 Rust、Tauri 2、Svelte 5、TypeScript、Tailwind 4、Bits UI / shadcn-svelte 和 Phosphor。

需要 Node.js 22、pnpm 11.7.0、Rust 1.95.0；Windows 另需 MSVC Build Tools 与 Windows SDK。

```sh
pnpm install --frozen-lockfile
pnpm desktop
```

`pnpm dev` 仅预览浏览器界面；截图、系统 OCR 与全局快捷键需在桌面程序中运行。

[开发指南](docs/v2.md) · [设计规范](DESIGN.md) · [验证记录](docs/validation.md) · [内存与体积](docs/performance.md) · [更新发布](docs/updating.md)

</details>

## 许可证

[MIT](LICENSE)。第三方资源与依赖的许可见 [Third-party notices](docs/v2-third-party-notices.txt)。
