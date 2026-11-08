# ScreenTranslate 2.0.29

- 修复 macOS 下载安装包打开提示“已损坏”的问题：对齐 Cuvilo 打包签名配置，在 `tauri.conf.json` 与发布流水线中显式启用完整的应用包 Ad-hoc 签名（`signingIdentity: "-"`、`hardenedRuntime: false`、`APPLE_SIGNING_IDENTITY: "-"`）并增加 `codesign --verify --deep --strict` 校验，确保首次下载打开直接弹出标准“从互联网下载的 App，确定要打开它吗？”确认框。
