# ScreenTranslate 2.0.34

- 修复 Linux GNOME 托盘已建立时仍可能随主窗口退出的问题，保持后台截图与快捷键服务继续运行。
- 修复 Ubuntu Wayland 快捷键格式不兼容导致的按键无效，并在系统未返回绑定结果时明确报告失败。
- 截图窗口拖动和缩放期间自动隐藏工具栏，操作结束后恢复，解决遮挡、残影和误点击。
- 修复 Wayland 首次拖动需要点击两次的问题，并将八个方向改为原生连续缩放，避免角点拉伸中断。
- 精简各平台 Release 资产：Windows 保留 NSIS，Linux 保留 AppImage 与 DEB，macOS 保留 DMG。
