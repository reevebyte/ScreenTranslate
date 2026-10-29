# ScreenTranslate

轻量的截图与文字翻译工具。框选文字，译文留在原处；也可以使用双栏文字翻译和快速翻译框。

当前版本 **2.0.2**。使用 Rust、Tauri 2、Svelte 5、TypeScript、Tailwind 4、Bits UI / shadcn-svelte 和 Phosphor。

## 使用

从 [GitHub Releases](https://github.com/reevebyte/ScreenTranslate/releases) 下载 Windows 可执行文件或 NSIS 安装包。Windows 10/11 需要系统 WebView2。

- 截图翻译：`Ctrl+Alt+Q`
- 收起 / 显示截图：`Ctrl+Alt+W`
- 快速文字翻译：`Ctrl+Alt+Space`
- 界面语言：设置 → 通用 → 界面语言，支持简体中文与 English。
- 修改快捷键：快捷操作页面直接录入组合键。
- 自动更新：设置 → 关于，默认启动时检查。标题栏提供下载与重启，托盘显示版本并可点击更新。
- 首次使用：自动打开使用指南，“开始使用”进入快捷操作。

截图支持原译对照、缩放、拖动、复制与文字校对。服务中心可配置翻译 API 和 OCR；查看服务不会自动启用它，点击“设为当前”后生效。API Key 使用系统凭据库，当前会话内容默认保存在内存中。

![服务中心](docs/screenshots/services.png)

![关于与更新](docs/screenshots/about.png)

以上为 Windows 原生程序客户区截图。

## 开发与构建

需要 Node.js 22、pnpm 11.7.0、Rust 1.95.0。Windows 使用 MSVC Build Tools 与 Windows SDK。其他平台依赖见 [开发指南](docs/v2.md)。

```sh
pnpm install --frozen-lockfile
pnpm desktop
```

`pnpm dev` 是浏览器界面预览，原生截图、OCR 与快捷键需要桌面程序。

```sh
pnpm check
pnpm test
cargo test --workspace --locked
pnpm build
node scripts/measure-v2.mjs
pnpm --filter @screentranslate/desktop tauri build --bundles nsis
```

正式可执行文件位于 `target/release`，安装包位于 `target/release/bundle/nsis`。构建成功后脚本清理更早的版本化产物，保留当前版本和活动构建缓存。

## 项目结构

- `apps/desktop/src`：界面、主题、语言、交互。
- `apps/desktop/src-tauri`：窗口、IPC、任务和更新入口。
- `crates/core`：翻译、会话、布局与配置。
- `crates/platform`：平台截图、OCR、凭据和旧配置迁移。
- `scripts`：构建、验证与安全清理。

## 文档

[开发指南](docs/v2.md) · [验证记录](docs/validation.md) · [内存与体积](docs/performance.md) · [OCR 自定义接口](docs/custom-ocr-api.md) · [更新发布准备](docs/updating.md) · [设计规范](DESIGN.md)

Windows 已进行原生验证。macOS / Linux 保留平台实现与 CI 构建配置，发布前仍需对应平台验收。本项目已配置专用更新公钥。发布签名更新前，需要设置 GitHub Actions 私钥 Secret；配置方法见[自动更新](docs/updating.md)。私钥仅在本机 `.local/` 与 GitHub Secrets 保管，不要上传到源码。

## 许可

[MIT](LICENSE)。第三方资源及依赖声明见 [THIRD-PARTY NOTICES](docs/v2-third-party-notices.txt)。
