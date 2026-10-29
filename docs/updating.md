# 自动更新与 GitHub 发布

ScreenTranslate 使用官方 Tauri updater 与 GitHub Releases，沿用 Cuvilo 的分阶段流程：启动 5 秒后静默检查、手动下载、签名校验、明确点击重启安装。关于页的“自动更新”默认开启，可关闭并自动保存；开发、浏览器和原生 E2E 构建不会自动联网检查。

检查和下载状态跨页面保留。下载失败可以重试，已验证安装包不会因为重新检查而丢失。未保存配置、正在截图/识别/翻译时禁止安装；重启确认会说明当前内存文字和截图会清空，设置保留。Windows 使用 passive NSIS 安装方式；生产环境强制 HTTPS 和签名验证。

## 本项目签名密钥

本次生成 ScreenTranslate 专用密钥；没有复用 Cuvilo 的密钥。

- 公开公钥：`apps/desktop/src-tauri/updater.pub`，已同步至 Tauri 配置并编入应用。
- 本机私钥：工作区 `.local/updater.key`。它被 Git 忽略且不进入源码包；请另作安全备份，不要随清理删除。
- 本机生成的私钥密码为空。后续可以按 Tauri 官方方式加密保管；不能随意更换已发行程序信任的公钥。
- 更新清单：`https://github.com/reevebyte/ScreenTranslate/releases/latest/download/latest-v2.json`。

2.0.0 普通构建尚未嵌入本项目公钥，需要先手动安装此 2.0.1 或更新版本；之后发布的签名版本才能由自动更新安装。

## 配置 GitHub

1. 在 `reevebyte/ScreenTranslate` 仓库进入 **Settings → Secrets and variables → Actions → New repository secret**。
2. 名称填写 `TAURI_SIGNING_PRIVATE_KEY`，值粘贴本机 `.local/updater.key` 的完整内容。不要把私钥提交到 GitHub 文件中。
3. `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 可留空，当前密钥没有密码。公钥已随源码提交，无需再设置变量；如使用 `SCREENTRANS_UPDATE_PUBLIC_KEY` 变量，其内容必须与已发行公钥相同。
4. 同步 Cargo、前端 package、Tauri 的正式版本号，创建并推送同版本标签，例如 `v2.0.3`。
5. `v2-release.yml` 会构建 Windows、macOS、Linux 草稿产物和签名，生成 `latest-v2.json`。检查后在 Releases 发布草稿，旧版便能发现更新。

2.x 更新清单独立于已退役的 1.x/Inno Setup，发布流程不再依赖旧版本网络清单。

没有配置私钥时，工作流只构建普通安装包草稿，不生成虚假的签名更新清单。切换 GitHub 仓库时还需要更新 Cargo、Tauri 配置和帮助链接中的仓库地址。

当前已验证本机签名下载、篡改拒绝、权限与安装条件。尚未上传 Secrets、推送标签或运行远端 Release；真实安装升级要在发布下一版本后验收。参考 [Tauri updater 官方文档](https://v2.tauri.app/plugin/updater/)。

Windows 标题栏的下载按钮不会增加 WebView；点击先下载，再次点击就绪按钮确认重启。托盘版本行在当前版本时显示为灰色，新版本可用时显示“↑ v版本 · 更新”，点击表示同意完整下载和安装流程；未保存配置或任务忙碌时禁用。

## 修复工作流后重试已有标签

如果旧标签的发布流程已经失败，先提交并推送工作流修复到 main。进入 Actions → ScreenTranslate draft release → Run workflow，选择 main，并在 tag 中填写已有标签（例如 v2.0.2）。这会使用 main 上修复后的流程构建该标签源码，并复用同标签草稿。不要只在旧失败任务点击 Re-run，否则仍会执行原来的流程；也无需删除或强制移动已有标签。

发布准备通过认证后的 release 列表定位草稿，并直接使用创建响应中的 release ID，不再在创建草稿后依赖按标签查询接口。已公开的 Release 继续受到保护，流程不会修改它。Windows runner 在检出前禁用换行转换，类型校验本身也将 CRLF 视为 LF；实际类型变化仍会报错。

发布整理脚本支持 `--target` 与 `--target=...`，Windows 独立 EXE 和清理范围跟随 `target/<平台>/release`。重试已有标签时，仅从当前工作流提交取构建驱动脚本，应用代码仍来自指定标签；不移动标签，也不更改已发布版本。

## 四个平台已构建成功，仅更新清单失败

提交并推送修复后，从 main 新运行发布工作流：tag 填 v2.0.2，并勾选“Only finish update metadata using the existing uploaded installers”。此模式跳过四个平台编译，复用草稿中已上传的安装包，只生成更新清单。

GitHub 草稿附件可能使用 untagged 临时地址。清单整理会读取认证后的附件目录，只接受同一 Release 的准确附件地址，核对每项 signature 与上传的同名 .sig 文件一致，再把下载地址规范为发布后的 v2.0.2 地址，并上传 latest.json 与 latest-v2.json。签名内容不变；跨仓库、其他草稿、缺失签名或签名不一致继续报错。最后任务变绿后才发布草稿。
