# ScreenTranslate（划词截屏翻译）

按下快捷键，在屏幕上框选一块区域，ScreenTranslate 会识别文字、翻译，并把译文覆盖回原位置。

ScreenTranslate 使用 **C++20 + 原生 Win32/C++/WinRT** 实现。发布产物是一个独立的 x64 EXE，静态链接 MSVC 运行库，不依赖 Python、Qt、.NET、Tauri、NuGet 或额外 DLL。

仅支持 Windows 10/11 x64。

在原文上进行直接翻译，也可以右键或空格显示原文
<img width="1630" height="931" alt="image" src="https://github.com/user-attachments/assets/37024f2e-2c5b-48c9-afbb-95f6e9eb3317" />

---

## 下载与启动

从本仓库的 [GitHub Releases](https://github.com/reevebyte/ScreenTranslate/releases) 下载：

- `ScreenTranslate-<版本>-setup-x64.exe`：安装版，可从应用内直接更新；
- `ScreenTranslate-<版本>-windows-x64.exe`：便携版，下载后直接运行。

Release 同时附带 `LICENSE.txt` 和 `THIRD_PARTY_NOTICES.txt`。它们不是运行依赖，但分发便携 EXE 时应一同提供。

安装版和便携版运行的是同一个原生程序。启动后程序常驻通知区域，默认快捷键如下：

| 快捷键 | 作用 |
| --- | --- |
| `Ctrl+Alt+Q` | 冻结屏幕并开始框选翻译 |
| `Ctrl+Alt+W` | 收起或重新显示上一次译文 |

左键单击托盘图标可以开始框选；右键菜单提供设置、检查更新、重启和退出。快捷键可在设置中修改。

本项目目前不使用 Authenticode 证书。Windows SmartScreen 可能显示“未知发布者”或首次下载警告，请只从上面的官方 Releases 页面下载。

## 主要功能

- 多显示器、物理像素坐标和 Per-Monitor V2 DPI 支持；
- 框选结束后重新截取干净画面，避免把选区遮罩截进 OCR 图片；
- Windows、Azure AI Vision 和有道云端 OCR；
- 12 个翻译后端，包括官方 API、免密网页接口和大模型接口；
- 结果窗口可移动、缩放、收起、复制、查看原文和重新翻译；
- 分块 OCR 校对，可只重译修改过的文本块；
- 后台任务取消、最多两个 worker，并只保留最新一次请求；
- 兼容既有 `config.json` 和 DPAPI 密钥格式；
- 基于 GitHub Releases 的更新检查、下载、SHA-256 校验和安装。

框选时按 `Esc` 或右键可取消。结果窗口支持 `E` 打开校对、`R` 重新翻译、`M` 收起、`Esc` 关闭；方向键可微调位置，按住 `Shift` 时步长为 10 像素。

## 翻译引擎

设置中的翻译接口可以随时切换：

| 接口 | 配置 | 说明 |
| --- | --- | --- |
| Microsoft Translator（官方） | Key、Region，可选 Endpoint | Azure 正式 API |
| Google Cloud Translation（官方） | API Key | Google Cloud 正式 API |
| Google 网页翻译（免密） | 无 | 开箱即用，网络可用性因地区而异 |
| Bing 网页翻译（免密） | 无 | 非正式开发者 API |
| Microsoft 移动接口（免密） | 无 | 模拟官方客户端请求 |
| 腾讯交互翻译（免密） | 无 | 网页接口 |
| Yandex 移动接口（免密） | 无 | 客户端接口 |
| 金山词霸（免密） | 无 | 适合中英文短文本 |
| DeepL | Key | 支持免费版和正式版 Endpoint |
| OpenAI 兼容接口 | Base URL、Key、Model | 可连接 DeepSeek、智谱、通义、Ollama 等兼容服务 |
| NVIDIA NIM | API Key、Model | NVIDIA 模型接口 |
| Anthropic | API Key、Model | Claude Messages API |

免密接口不等于服务商承诺稳定的公开 API，可能限流、改版或停止工作。正式或高频用途应优先选择官方 API。

## 文字识别

| OCR 引擎 | 配置 | 数据与适用场景 |
| --- | --- | --- |
| Windows OCR | 无 | 本地、免费，常规界面和文档通常最快，默认选择 |
| Azure AI Vision Read | Endpoint、Key | 上传选区截图，适合更多字体和语言 |
| 有道云端 OCR | 无 | 上传选区截图，免密但属于非官方接口 |
| RapidOCR | 可选插件 | 原生宿主接口已完成，但官方 Release 尚未捆绑 OCR 插件和模型 |

Azure OCR 需要 Azure AI Vision 或支持 Vision 的多服务资源；单独的 Translator 密钥通常不能直接用于 Vision。云端 OCR 会发送完整框选截图，画面可能包含账号、聊天和文件名等敏感信息，敏感内容请使用 Windows OCR。

Windows OCR 能识别的语言取决于系统已安装的 OCR 语言包。可在“Windows 设置 → 时间和语言 → 语言和区域 → 语言选项 → 可选功能”中添加。

## 配置与隐私

配置文件位置：

```text
%APPDATA%\ScreenTranslate\config.json
```

程序直接读取既有配置。API 密钥使用当前 Windows 账户的 DPAPI 加密，磁盘中的值以 `dpapi:v1:` 开头；换到其他账户或电脑后无法解密，需要重新填写。

DPAPI 可以降低误传配置文件造成的泄露风险，但同一 Windows 账户下运行的其他程序仍可能解密。不要把配置文件、日志或带有密钥的截图提交到仓库。

如果开启“翻译后自动复制”，译文还可能进入 Windows 剪贴板历史或跨设备同步。处理敏感文字时应关闭该选项以及系统剪贴板历史。

## 构建原生版

需要：

- Visual Studio 2022 或更新版本；
- “使用 C++ 的桌面开发”工作负载；
- Windows 10/11 SDK。

在仓库根目录运行：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass `
  -File .\scripts\build-native.ps1 `
  -Configuration Release `
  -RunSmokeTest
```

产物：

```text
build\native\Release\ScreenTranslate.exe
```

`-RunSmokeTest` 会执行程序生命周期自检和更新器自检，不会留下后台进程。还可以验证最终 PE 文件的架构、版本资源、GUI 子系统以及是否意外依赖动态 CRT：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass `
  -File .\release\check_native_binary.ps1 `
  -Path .\build\native\Release\ScreenTranslate.exe `
  -ExpectedVersion 1.0.3
```

Debug 构建：

```powershell
.\scripts\build-native.ps1 -Configuration Debug
```

构建脚本会自动查找 Visual Studio 和它自带的 CMake，无需打开 Developer PowerShell。

## 自动构建与发布

- `.github/workflows/windows-ci.yml`：提交和拉取请求时构建、自测并校验原生 x64 单 EXE；
- `.github/workflows/windows-release.yml`：推送 `v<SemVer>` 标签后构建原生 EXE、Inno Setup 安装器和 `update-manifest.json`，再创建 GitHub Release。

Release 工作流只允许从 `reevebyte/ScreenTranslate` 发布，因为程序内的更新信任根固定为这个仓库。版本同步、构建和产物检查均由仓库中的发布工具与工作流完成。

## 应用内更新

程序只信任：

```text
https://github.com/reevebyte/ScreenTranslate
https://github.com/reevebyte/ScreenTranslate/releases/latest/download/update-manifest.json
```

下载地址必须与同一仓库、`v<version>` 标签和 `ScreenTranslate-<version>-setup-x64.exe` 文件名完全一致。安装前会校验清单结构、版本、通道、文件大小和 SHA-256；下载缓存和更新辅助进程还会再次校验路径及文件内容。

更新不会静默绕过 Windows 安全提示。GitHub 在部分网络环境中可能访问较慢，失败时可稍后重试或从 Releases 页面手动下载。

## 目录结构

```text
native/                 原生 C++20 / Win32 / C++/WinRT 客户端
scripts/build-native.ps1 原生构建入口
installer/              Inno Setup 安装器
release/                版本、PE 和更新清单校验工具
.github/workflows/      原生 CI 与 Release 流程
```

## 已知限制

- 只支持 Windows 10/11 x64；
- RapidOCR 原生宿主接口已完成，但官方包尚无插件实现和模型；没有自行部署插件时应选择 Windows、Azure 或有道 OCR；
- Windows OCR 依赖系统语言包，艺术字、竖排字和手写体的识别率有限；
- 免密翻译和有道 OCR 不是稳定性受保证的开发者 API；
- 复杂照片背景上的原文修补和译文排版仍可能不如纯色界面自然；
- 当前发布包没有代码签名证书，SmartScreen 警告属于预期行为。

## 许可证

ScreenTranslate 自有代码采用 [MIT License](LICENSE)。发布版使用 Windows SDK 提供的系统接口，不捆绑 Qt、Python 或第三方运行时；来源声明见 [native/THIRD_PARTY_NOTICES.txt](native/THIRD_PARTY_NOTICES.txt)。
