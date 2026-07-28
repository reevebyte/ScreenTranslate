# 发布说明

## 首次上传 GitHub

更新检查需要匿名访问 GitHub Release，因此准备给普通用户使用时，仓库应设为 **Public**。
Private 仓库的 Release 和更新清单需要登录授权，当前应用不会收集 GitHub 凭据，也就无法为
普通用户检查更新。公开前先确认源码、提交历史和待提交文件中没有 API 密钥、个人配置、日志、
截图或其他隐私数据。

先登录 GitHub，在网页右上角选择 **New repository**，仓库名建议使用 `ScreenTranslate`。
创建时不要勾选初始化 `README`、`.gitignore` 或 License，因为本地项目已经包含这些文件；
否则首次推送会产生两套不相干的提交历史。创建完成后先不要在网页上传文件。

Git 提交会永久记录提交者姓名和邮箱。可以先查看当前设置：

```powershell
git config --global --get user.name
git config --global --get user.email
```

如果不希望真实邮箱出现在公开提交历史中，在 GitHub 的 **Settings > Emails** 开启邮箱隐私，
使用页面提供的 `ID+用户名@users.noreply.github.com` 地址。以下 `git config` 不带 `--global`，
只修改当前仓库，不影响电脑上的其他项目：

```powershell
cd F:\project\claudeproject\translate
git init -b main
git config user.name "你的 GitHub 用户名"
git config user.email "你的 GitHub noreply 邮箱"
```

首次提交前先检查 Git 实际准备上传的文件。`git status` 中不应出现本地密钥、配置、日志、
虚拟环境、`build` 或 `dist` 产物；发现异常时先完善 `.gitignore`，不要急着提交：

```powershell
git add .
git status
git commit -m "Initial release"
git remote add origin https://github.com/<GitHub用户名>/ScreenTranslate.git
git push -u origin main
```

HTTPS 首次推送通常会由 Git Credential Manager 打开浏览器登录 GitHub。GitHub 不接受账户
密码作为命令行 Git 密码；浏览器登录不可用时，应使用权限最小化的 Personal Access Token。
不要把 Token 写进远程地址、源码、脚本或提交记录。

源码推送成功后，确认 `screentrans/__init__.py` 中的版本为 `1.0.0`，并确认 GitHub Actions
中的 Windows CI 已通过，再创建首个带说明的版本标签：

```powershell
git status
git tag -a v1.0.0 -m "ScreenTranslate 1.0.0"
git push origin v1.0.0
```

推送 `v1.0.0` 会触发 Release 工作流。流水线会自动创建 GitHub Release 并上传安装器、
便携 ZIP 和 `update-manifest.json`，不要提前在网页创建同名 Release。完成后在仓库的
**Releases** 页面核对三个资产是否齐全，并从 Release 页面实际下载、安装和检查一次更新。

## 可复现环境

正式 Windows 构建固定使用 CPython 3.11 x64，并分成四套依赖文件：

| 文件 | 用途 |
| --- | --- |
| `requirements.txt` | 开发时可升级的兼容版本范围 |
| `requirements.lock` | 基础运行时精确版本 |
| `requirements-build.lock` | 基础运行时加 PyInstaller 构建工具精确版本 |
| `requirements-rapidocr.lock` | 可选 RapidOCR 运行时精确版本 |
| `requirements-audit.lock` | CI 使用的 pip-audit 及其依赖精确版本 |

新建虚拟环境后执行：

```powershell
python -m venv .venv-build
.\.venv-build\Scripts\python.exe -m pip install pip==26.1.2
.\.venv-build\Scripts\python.exe -m pip install -r requirements-audit.lock
.\.venv-build\Scripts\python.exe -m pip check
.\.venv-build\Scripts\python.exe release/check_environment.py --strict-lock
.\.venv-build\Scripts\python.exe -m pip_audit -r requirements.lock --disable-pip --no-deps --cache-dir .pip-audit-cache
.\.venv-build\Scripts\python.exe -m pip_audit -r requirements-rapidocr.lock --disable-pip --no-deps --cache-dir .pip-audit-cache
```

不要为了解决依赖冲突直接修改系统或全局 Python 环境；删除并重建工作区内的
`.venv-build` 即可获得干净构建环境。测试、`build.py` 和源码自检都必须显式使用
`.\.venv-build\Scripts\python.exe`，否则 PowerShell 仍可能调用全局 Python。

锁定文件不要由装有其他项目依赖的全局环境执行 `pip freeze` 生成。升级时应在干净的
Python 3.11 x64 虚拟环境中解析、测试，然后同时更新兼容范围和对应锁定文件。
基础运行时不包含 NumPy。NumPy 只属于 `requirements-rapidocr.lock`；在升级可选
RapidOCR/OpenCV 依赖时，必须同时验证它与 PySide6/shiboken6 的二进制兼容性，并运行
RapidOCR 构建的导入测试和打包自检。

## 本地构建

```powershell
.\.venv-build\Scripts\python.exe -m unittest discover -s tests -p "test_*.py" -v
.\.venv-build\Scripts\python.exe build.py
dist\ScreenTranslate\ScreenTranslate.exe --selftest
```

CI 使用 `--selftest --quiet --offline` 启动刚生成的 EXE；它仍会验证 OCR、一次真实识别、
排版渲染、翻译模块装载和全局快捷键，但不会访问翻译接口或要求发布机保存真实 API 密钥。

基础构建会明确排除 NumPy 和 RapidOCR 依赖；使用 `--with-rapidocr` 时，`build.py` 会在
清理旧产物之前检查 NumPy ABI，不兼容时返回退出码 2。

## 标签发布

推送 SemVer 标签会触发 `.github/workflows/windows-release.yml`：

```powershell
git tag v1.2.3
git push origin v1.2.3
```

打标签前必须先把 `screentrans/__init__.py` 中的 `__version__` 改为同一版本。
流水线会执行 `release/check_version.py`，不一致时直接拒绝发布，避免安装器、清单和程序
自身版本互相矛盾。

流水线依次执行锁定环境检查、回归测试、PyInstaller 构建、内置更新源一致性检查、打包后离线自检、
Inno Setup 6.7.1 安装器构建、便携 ZIP 打包、更新清单生成和 GitHub Release 创建。
构建任务只有仓库只读权限，且 checkout 不保存凭据；只有下载已验证产物并创建 Release 的
独立任务具有 `contents: write` 权限。Dependabot 每周检查 Python 与 GitHub Actions 更新，
CI 同时审计基础版及可选 RapidOCR 的锁定依赖。
当前发布方案不使用 Authenticode 代码签名证书，也不需要在 GitHub Secrets 中保存 PFX
或密码。Windows SmartScreen 因而可能在首次运行时显示“未知发布者”；发布说明中必须如实
提醒用户，并建议用户只从本仓库的 GitHub Release 页面下载。

带预发布标识的 SemVer 标签，例如 `v1.2.3-rc.1` 或 `v1.2.3-beta.1`，会被流水线自动标为
GitHub prerelease，并生成 `preview` 通道清单。最终版本标签生成 `stable` 通道清单。
GitHub 的 `releases/latest` 不包含 prerelease，因此稳定版用户不会收到 RC 或 beta 提示。

安装器把每个版本放在独立的 `{app}\versions\<version>` 目录中。新版本文件全部复制成功后
才删除旧版本目录，并且从旧布局升级时只清理明确的 `ScreenTranslate.exe` 与 `_internal`；
用户把安装位置选到已有目录时，不会通配删除目录中的其他文件。

## 更新清单

`release/generate_manifest.py` 只接受同一 GitHub 仓库、标签和文件名完全匹配的 Release
下载地址以及 SemVer 版本，并计算实际安装器的字节数和 SHA-256。示例：

```powershell
python release/generate_manifest.py `
  --artifact dist/ScreenTranslate-1.2.3-setup-x64.exe `
  --version 1.2.3 `
  --download-url https://github.com/OWNER/REPO/releases/download/v1.2.3/ScreenTranslate-1.2.3-setup-x64.exe `
  --output dist/update-manifest.json `
  --channel stable
```

GitHub 最新正式版清单可使用稳定地址：
`https://github.com/OWNER/REPO/releases/latest/download/update-manifest.json`。

GitHub Actions 构建时，`build.py` 会从 `GITHUB_REPOSITORY` 自动生成仓库主页和上述稳定
清单地址，并作为只读资源嵌入程序；不需要在源码中硬编码 OWNER/REPO。正式包启动后会用
该资源覆盖磁盘上可能残留的旧更新地址。源码运行和没有 `GITHUB_REPOSITORY` 的本地构建
默认不配置更新源，不会自行联网检查。

Release 流水线在运行打包自检前还会读取产物中的
`dist/ScreenTranslate/_internal/screentrans-update-source.json`。其中仓库地址必须与当前
`GITHUB_REPOSITORY` 一致，通道必须与标签计算出的 `RELEASE_CHANNEL` 一致，清单必须精确为
同一仓库的 `releases/latest/download/update-manifest.json`；缺少文件、字段重复、额外字段
或任一值不一致都会阻断发布，避免正式包意外成为“无更新源”或指向其他仓库的版本。

`screentrans/updater.py` 和 `screentrans/ui/update_dialog.py` 只检查版本并打开系统浏览器：
它们不下载、不保存、也不运行 EXE、ZIP 或其他 Release 资产。清单限制为 256 KiB，初始
地址必须是 GitHub Release 的 `update-manifest.json`，重定向只能留在 GitHub 的资产基础
设施中。返回的发布页面以及安装包元数据必须精确匹配内置仓库、`v<version>` 标签和文件名，
大小与 SHA-256 也必须合法；更新窗口会显示这些信息供用户核对。即使清单内容被错误修改，
应用内也没有把远程文件带入执行链的能力。用户点击“打开发布页面”后，由系统浏览器访问
同一官方仓库并手动下载；主控制器不会为了更新而退出当前程序。

稳定通道只接受最终版本。预览通道允许比较预发布版本，也允许后续最终版本覆盖当前 RC；
不过 `releases/latest` 本身只指向正式版本，测试人员仍应在 GitHub 的 Releases 页面关注新的
RC。程序退出路径会调用更新窗口的 `shutdown()`，取消尚未完成的清单请求并作有界等待。

## 本地错误日志的接入约定

`screentrans/error_logging.py` 提供 512 KiB、3 个备份的 UTF-8 轮转错误日志，默认位置为
`%LOCALAPPDATA%\ScreenTranslate\logs\errors.log`。格式器会脱敏常见 Authorization、
API key、token、DPAPI 密文、URL 查询凭据和内嵌图片 data URL。

`run.pyw` 已安装 `sys.excepthook` 和 `threading.excepthook`，同时保留 Python 原有异常
处理行为。OCR/翻译 worker、结果渲染、重新截图和更新操作的失败出口也已接入；用户界面
仍显示原来的具体错误。`report_exception` 只落盘操作名、异常类型和代码栈位置，刻意省略
可能携带接口响应或用户文本的异常消息，不记录截图、OCR 原文、译文、接口请求/响应体或
配置对象。正常翻译与用户输入不记日志。
