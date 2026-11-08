# ScreenTranslate 2.0.31

- 修复 Linux (Wayland / Ubuntu GNOME) 截图后无法返回翻译的问题：当系统截图工具完成抓屏而桌面 Portal 文档网关未返回可读文件路径时，自动从系统剪贴板或系统截图目录（`~/Pictures/Screenshots`、`~/图片/截图`）接管刚生成的截图并进入识别翻译流程。
- 优化 Linux 截图取消体验：在系统截图界面取消操作时静默恢复主窗口，不再误弹红色错误提示。
- 启用 macOS 应用包完整 Ad-hoc 签名与结构校验（`signingIdentity: "-"`、`hardenedRuntime: false`），解决首次下载打开误报“已损坏”的问题。
