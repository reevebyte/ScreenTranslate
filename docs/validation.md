# 2.0.4 验证记录

- 原因：隐藏窗口在后台 60 秒后被定时回收（轮询间隔 15 秒）。回收最后一个窗口会触发 Tauri 的无退出码退出请求，原先未拦截，导致托盘进程一起结束。
- 修复：阻止无退出码的隐式退出，继续回收隐藏 WebView；显式退出与更新重启仍由带退出码的请求处理。
- 全局代码审查与边界修复：
  - `pipeline::retranslate_block` 先校验目标文本块属于当前截图会话，避免旧截图校对请求误取消正在进行的新截图识别。
  - `windows::begin_capture` 仅在当前截图任务仍为最新票据时回退错误，避免已废弃截图的迟到错误关闭新选区。
  - `commands::refresh_capabilities` 保留启动时的实际托盘可用状态。
  - `HttpTranslator::new` 将模型非空校验推迟到实际翻译时执行，允许新接入的兼容 AI 服务先拉取模型列表再选模型。
  - `layout::normalize_ocr_text` 保留纯文本 OCR 返回的换行符，避免多段文本被压成单行。
  - `QuickView` 修复组件提前卸载时异步事件监听未注销的问题；`ResultView` 避免空格键抢占工具栏按钮激活或误发状态更新。
  - 清理未使用的 `render::repair_text_region` 及多处前端死变量与未用导入。
- Windows 原生回归通过：使用临时配置与真实 Wry WebView，连续两轮等待生产回收计时器清空所有窗口，再分别重开 quick/main；会话保留，最后显式退出成功。父进程核对完整结束标记，避免运行时提前以 0 退出掩盖失败；用时约 151 秒。
- 原生回归命令：`cargo test -p screentranslate-desktop --lib --features e2e background_tests::reaped_windows_keep_background_runtime_alive -- --ignored --nocapture --test-threads=1`。
- 修正 Windows lib 单元测试缺少 Common Controls v6 清单的问题；正式程序继续使用 Tauri 自带的完整应用清单。
- Rust fmt、Clippy（`-D warnings`）、63 项 Rust 测试、前端 svelte-check（0 错误 0 警告）、71 项前端测试、29 项发布与清理脚本测试全部通过；生产前端与体积预算检查通过。
- Windows NSIS 正式包构建成功：`ScreenTranslate_2.0.4_x64-setup.exe`，3,290,273 bytes（约 3.14 MiB）；文件版本与产品名核对通过，成品仍包含应用清单和 Common Controls 依赖。SHA256 位于本机 `build/release-2.0.4/artifacts.json`。
- 构建成功后由 `scripts/tauri-v2.mjs` 清理旧版产物，当前安装包、可执行文件与活动编译缓存保留。
- 本轮原生回归覆盖后台窗口回收和事件循环，不代表实际按键、托盘点击或更新安装已做端到端验收。未改动用户配置、服务密钥，未发布远端版本。
# 2.0.3 验证记录

- 产品名统一为 ScreenTranslate；版本号独立为 2.0.3，应用 identifier 与更新公钥保持不变。
- 图标源改为 SVG 路径，使用 Tauri icon 重新生成 ICO/ICNS/PNG。ICO 含 16、24、32、48、64、256px，均保留透明角，SVG 不再内嵌旧位图。
- 许可证与第三方说明移到 licenses 子目录；NSIS 增加针对旧 ScreenTranslate 2 名称的迁移，调用其卸载器并保留应用数据及凭据。
- check 无错误/警告；前端 68 项、发布脚本 29 项通过；生产前端与体积预算检查通过。
- 新增并通过独立原生首次启动用例：临时空配置自动打开指南，开始使用后进入快捷操作，完成标志保存，重新显示主窗口不重复弹出。图片位于本机 build/release-2.0.3/native-first-run/。
- 用户现有配置不作重置；已有 onboardingComplete=true 的机器重装后不重复引导，可从设置 → 关于 → 使用指南重新打开。
- 本轮未执行对用户已安装程序的实际升级/卸载，安装迁移仍需对应环境验收。
- Windows NSIS 正式包构建成功：ScreenTranslate_2.0.3_x64-setup.exe，约 3.13 MiB；构建成功后的旧产物清理由项目脚本执行。

# 2.0.2 验证记录

Windows、本机 WebView2 和当前 DPI。原生 E2E 使用独立标识、临时配置、本机翻译服务和合成图片；不会覆盖系统剪贴板或用户服务密钥。没有推送 GitHub 或执行更新安装。

## 修正与核对

1. 关于页隐藏装饰滚动轨道；保留滚轮/键盘滚动，600×400、150% 字体仍能访问内容。
2. 托盘新增当前版本行。当前版本为禁用灰色，新版显示“↑ v版本 · 更新”，可点击完整下载和安装；检查中、下载中、安装中和工作忙碌时禁用。
3. Windows 标题栏复用 Cuvilo 的轻量原生控件思路，提供下载、进度、重启与关闭提示。控件不创建 WebView，不改变客户区尺寸；遵循 DPI 和标题栏颜色。
4. 关于页和标题栏重启确认共用一个按需加载的 Dialog。托盘直接更新也须通过未保存设置、截图和翻译忙碌保护。
5. 原生请求在主窗口释放/重建期间排队保存；下载期间不释放主窗口。已验证的更新包留在原生状态，重开后可以接续。
6. 新截图呈现后释放更早的缓存。延迟到达的呈现事件不得删除新截图，增加对应回归。当前译图仅保留 PNG，去掉重复的 RGBA 像素缓存。
7. 正式包使用 opt-level z、LTO、单代码生成单元、符号去除和未使用插件命令裁剪；保留 panic 展开恢复，不打包 WebView2 运行时。NSIS 使用 LZMA。
8. Git origin 为 https://github.com/reevebyte/ScreenTranslate.git；Git 作者为 reevebyte / mo0nster@163.com，当前 GitHub 连接资料一致。只读核对，不更改凭据或执行上传。

## 验证

- 前端 68 项通过；check 无错误/警告；types:check、fmt、clippy、构建预算检查通过。
- Rust 包括缓存重复替换、晚到事件保留新截图、更新权限/忙碌保护、真实 HWND 客户区不变、DPI、捕获属性和销毁、签名下载与篡改拒绝。4 项需实际 OCR/联网环境的旧测试仍按原配置忽略。
- 原生桌面 5、服务 4、窗口/截图/校对 15、拉伸 2、更新控件 2，合计 28 项通过；更新控件显隐/下载/重启状态不改变主客户区与位置。
- 浏览器合成预览覆盖浅/深色、中英文、减少动态、600×400/150%、按需加载确认弹窗、Escape 与滚动，无页面异常。不能代替原生窗口或安装验证。
- computer-use 观察尝试未取得可靠标题栏画面，不作为人工视觉验收通过；原生控件以实际 HWND 集成测试和原生几何回归为依据。
- 内存测量为本机正式程序冷启动后 3/8/15 秒，汇总主程序及 WebView2 子进程；没有强制清空工作集。对比及各进程原始数值保存在 build/release-2.0.2/memory-*.json。共享工作集总和可能重复计数，不能当作独占物理内存。
- 2.0.1 安装包 3,912,235 bytes；初次 2.0.2 包 3,344,909 bytes。最终产物大小和 SHA256 记录在本机 artifacts.json。

只在 Windows 当前显示环境测试；尚未本轮验收 macOS/Linux 或远端 GitHub Release 的实际升级安装。私钥保留于 .local，源码包和 Git 均排除它；保留历史源码恢复归档与活动构建缓存。

最终 Rust 59 项通过、4 项忽略；清理与更新清单脚本 5 项通过。正式安装包再次完成下载验签和篡改拒绝验证，未执行安装。最终内存与体积数据见 [内存与体积](performance.md)。
