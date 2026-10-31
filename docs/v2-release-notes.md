# ScreenTranslate 2.0.9

- 标题栏更新按钮引入 GDI 内存双缓冲与 `WM_SETTEXT` 拦截，消除下载进度流式更新时的背景频闪。
- 标题栏“重启更新”按钮点击直接启动安装器并重启应用，消除无响应问题。
- 升级 GitHub Actions macOS runner 镜像至 macos-15，确保多平台发布自动化稳定运行。
