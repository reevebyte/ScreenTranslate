# ScreenTranslate 2.0.40

- 新增可选的高精度离线本地 OCR 引擎 **PP-OCRv5 本地**（基于独立开源库 [LumenOCR](https://github.com/reevebyte/LumenOCR)）：支持中 / 英 / 日多语种高精度离线识别，在「服务中心 → 文字识别 → 本机」按需下载并强制校验 SHA-256，不增加默认安装包体积。
- 本地 PP-OCRv5 推理在独立 `--ocr-worker` 子进程中运行，连续截图热复用，支持配置空闲自动释放内存（默认 3 分钟）或一键释放 / 彻底删除模型文件，确保零常驻内存膨胀。
- 完善 Linux 桌面与 Wayland 体验：新增原生 StatusNotifierItem 托盘支持、平铺 Wayland 窗口管理器（Hyprland / Sway / Niri）快捷键与截图适配，以及 Linux 桌面安装脚本与 PKGBUILD。
- 优化 macOS 与 Linux 截图、结果工具栏位置与边缘自适应、本地 OCR 语言回退提示，并修复取消模型下载误显报错等细节问题。
