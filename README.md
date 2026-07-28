# ScreenTranslate（划词截屏翻译）

按一下快捷键，在屏幕上拉一个框，松手——译文立刻盖在原文上。

- 框选的是**外语** → 翻译成**中文**
- 框选的是**中文** → 翻译成**英文**（目标语言可改）

语种自动判断，界面上没有任何按钮，只有一个框。

---

## 安装

```bash
python -m pip install -r requirements.lock
```

锁定文件用于获得与 CI、正式构建一致的环境；`requirements.txt` 则保留了兼容版本范围，
只适合开发时有意识地升级依赖。当前正式构建使用 Python 3.11 x64，仅支持 Windows 10/11。

基础版不依赖 NumPy，也不会加载 OpenBLAS。只有可选 RapidOCR 需要 NumPy；它的锁定文件
使用 NumPy 1.26，与当前 PySide6 6.5 的二进制接口保持兼容。

## 启动

双击 `run.bat`，或者：

```bash
pythonw run.pyw
```

启动后常驻系统托盘（右下角的绿色气泡「译」字图标）。

| 全局快捷键 | 默认 | 作用 |
| --- | --- | --- |
| 框选翻译 | `Ctrl+Alt+Q` | 冻结屏幕，进入框选 |
| 收起 / 显示 | `Ctrl+Alt+W` | 同一个键来回切：译文开着就收进托盘，收着就叫回来 |

两个都能在设置里改。左键单击托盘图标 = 立刻开始框选；右键 → 设置 / 重启 / 退出。

> Windows、Azure AI Vision 和有道云端 OCR 之间切换会立即生效。只有**切换到 RapidOCR**，
> 或者刚安装 RapidOCR / Windows OCR 语言包时需要重启。可用托盘右键「重启」，
> 或者设置 → 其他 →「重启程序」。

## 用法

### 框选

| 操作 | 结果 |
| --- | --- |
| 移动鼠标（尚未按下） | 鼠标中心只显示一对短十字线段，不再有贯穿整块屏幕的延伸线 |
| 拖动鼠标 | 画出要翻译的区域 |
| 松开左键 | 立刻识别并翻译，译文原地覆盖 |
| `Esc` / 右键 | 取消框选 |

### 译文窗口

四个按钮（校对 / 重译 / 收起 / 关闭）做成一根小条，贴在选区**右下角外侧**——
不占选区里的一个像素，所以不会挡住任何译文。

| 操作 | 结果 |
| --- | --- |
| 右下角 `✕` / `Esc` | 关闭 |
| 右下角铅笔 / `E` | 打开逐块 OCR 校对窗口；识别或翻译完成前不可用 |
| 右下角 `↻` / `R` | **用当前接口重新翻译**（文字不重认，只重翻） |
| 右下角 `—` / `M` / `Ctrl+Alt+W` | 收到托盘，之后同一个键或托盘菜单「显示上次译文」叫回来 |
| **拖动那根小条** | **移动窗口**（条左端那几个点就是拖拽手柄） |
| 方向键 | 微调位置，按住 `Shift` 一次挪 10 像素 |
| **拖动四条边或四个角** | 扩展/收缩框选区域；松手后重新截图、识别并翻译，画面不会拉伸变形 |
| 拖动时按住 `Shift` | 锁住当前长宽比 |
| 双击空白处 / `Home` | 回到刚框选时的位置和大小（双击文字上还是选词） |
| **在译文上拖动** | **划选文字**（选中部分会高亮） |
| 点画面空白处 | 取消当前划选 |
| `Ctrl+C` | 复制：划选了就复制选中的，没划选就复制全部 |
| `Ctrl+A` | 复制全部译文 |
| 按住空格或右键 | 临时看回原文 |

> 默认会在每次翻译成功后把完整译文自动复制到剪贴板，可在设置 → 显示中关闭。
> 如果 Windows 已开启剪贴板历史（`Win+V`）或跨设备同步，译文可能被历史记录保留，
> 或同步到登录同一 Microsoft 账户的设备。处理敏感内容时建议关闭自动复制和系统剪贴板历史。

校对窗口左侧按屏幕顺序列出每个文本块，右侧可修改该块的识别原文和目标语言。
OCR 引擎提供置信度时会显示真实数值，并标出低于 75% 的块建议重点校对；引擎不提供时
会明确写出“不提供置信度”，不会用估算值冒充。点「应用并重译」只请求当前块，其他
块的译文保持不动；也可以先修改多个块，再点「应用其余更改」只批量重译这些改动块。

关闭只有 `✕` 和 `Esc` 两条路——点画面任意处、或者切到别的窗口，都**不会**让译文消失，
免得刚翻完还没看就被误触点没了。嫌它挡路就 `—` 收起来，或者拖控制条挪开。

> 画面内部**不**响应拖动移动窗口：那一层要留给划选文字，两件事抢同一个左键拖拽会打架。
> 移动窗口请拖右下角那根控制条，或者用方向键。

> 窗口贴着屏幕最下面时，那根小条放不下，会自动翻到框内贴着底边，不会跑出屏幕。

## 首次配置

程序默认使用**微软 Azure 翻译**，需要填一个密钥才能用：

1. 打开 [Azure 门户](https://portal.azure.com) → 创建资源 → 搜索 **Translator** → 创建（选 **F0 免费层**，每月 200 万字符）
2. 进入该资源 → 左侧「密钥和终结点」
3. 复制 **密钥 1** 和 **位置/区域**（如 `eastasia`）
4. 托盘图标右键 → 设置 → 翻译引擎，把两项填进去
5. 点「测试连接」，显示绿色 `OK -> 你好，世界。` 就成了

## 可选的其他翻译接口

设置里的「接口」下拉框可随时切换：

| 接口 | 需要什么 | 说明 |
| --- | --- | --- |
| 微软 Azure 翻译 | Key + Region | 免费额度大，国内直连，**默认** |
| 谷歌翻译（官方 API） | API Key | 需要 Google Cloud 项目 |
| 谷歌翻译（免密钥） | 无 | 开箱即用，但国内通常要代理 |
| 必应翻译（免密钥） | 无 | 使用必应网页接口 |
| 微软翻译（免密钥） | 无 | 模拟微软客户端请求，不是 Azure 正式 API |
| 腾讯交互翻译（免密钥） | 无 | 使用腾讯交互翻译网页接口 |
| Yandex 翻译（免密钥） | 无 | 使用 Yandex 客户端接口，国内可用性不固定 |
| 词霸翻译（免密钥） | 无 | 适合中英文短文本 |
| DeepL | Key | 译文质量高，免费版每月 50 万字符 |
| AI 大模型（OpenAI 兼容） | base_url + Key + model | 见下表，翻译质量最好 |
| 英伟达 NIM | API Key | 地址已预置，只需填 Key，新账号有免费额度 |
| Claude（Anthropic） | API Key + model | 使用 Messages API |

标为“免密钥”的接口模拟公开网页或官方客户端，并不是服务商承诺稳定的开发者 API。
它们可能限流、改版或随时失效，也可能受各服务商使用条款限制；正式或高频用途应优先选官方 API。

英伟达 NIM 的 Key 到 [build.nvidia.com](https://build.nvidia.com) 领（挑任一模型 → Get API Key，
形如 `nvapi-xxxx`）。

**AI 类接口的「模型」是一个下拉框**：填好 Key 之后点右边的「刷新」，
程序会调 `/v1/models` 把该账号真正可用的模型拉下来给你选，也可以直接手输。
模型名写错时这些接口通常只回一句 `404 page not found`，光看报错猜不到问题在模型上，
所以别硬猜——点一下刷新最省事。

「AI 大模型」这一项填不同的 `base_url` 就能对接绝大多数服务：

| 服务 | base_url | model |
| --- | --- | --- |
| DeepSeek | `https://api.deepseek.com/v1` | `deepseek-chat` |
| 智谱 GLM | `https://open.bigmodel.cn/api/paas/v4` | `glm-4-flash` |
| 通义千问 | `https://dashscope.aliyuncs.com/compatible-mode/v1` | `qwen-plus` |
| 月之暗面 | `https://api.moonshot.cn/v1` | `moonshot-v1-8k` |
| 硅基流动 | `https://api.siliconflow.cn/v1` | `Qwen/Qwen2.5-7B-Instruct` |
| OpenAI | `https://api.openai.com/v1` | `gpt-4o-mini` |
| 本地 Ollama | `http://localhost:11434/v1` | `qwen2.5:7b` |

## 文字识别

默认用 **Windows 自带的 OCR**：完全离线、免费、几十毫秒出结果。设置中还可选择：

| OCR 引擎 | 配置 | 数据与适用场景 |
| --- | --- | --- |
| 系统自带 OCR | 无 | 完全离线，识别常规界面和文档最快，**默认** |
| Azure AI Vision OCR | Endpoint + Key | 官方云端 Read API；上传框选截图，适合更多字体和语言 |
| 有道云端 OCR | 无 | 上传框选截图；使用非官方客户端接口，可能限流或失效 |
| RapidOCR | 可选本地依赖 | 完全离线，艺术字更强，但体积更大、速度更慢 |

Azure OCR 需要 Azure AI Vision（或支持 Vision 的多服务）资源的 Endpoint 和 Key；已有的
Translator 单服务密钥通常不能直接当作 Vision 密钥使用。云端 OCR 会把**整个框选截图**发送给
对应服务，截图可能包含账号、聊天、文件名等敏感信息；敏感画面请使用 Windows OCR 或 RapidOCR。
Azure 的数据处理以你的 Azure 资源条款为准，有道接口则不是正式开发者 API，不提供稳定性保证。

选「自动」时，配置里的每种语言都会**真跑一遍**，再按质量分挑赢家。
不这么做不行：中文引擎读英文界面时会读出**更多**字符，但全是坏的——
`o` 认成 `0`（`Project` → `Pr0Ject`）、图标幻觉成孤立汉字（`囗`、`匚`）、
`river` 切成 `ri ve r`。只比「认出多少字」的话，垃圾反而赢。
质量分按词记：汉字和字母粘在一个词里、词中间夹数字、一两个字母的碎片，都不给满分。

系统当前装了哪些 OCR 语言，设置界面的「文字识别」里会直接列出来。要加日语、韩语等：

> Windows 设置 → 时间和语言 → 语言和区域 → 添加语言 →
> 点该语言的 ⋯ → 语言选项 → 可选功能 → 添加「光学字符识别」

装好后重启本程序即可。

### 艺术字识别不出来？换 RapidOCR

系统 OCR 是照着文档和界面文字训练的，遇到**视频封面、海报那种花体字/描边艺术字**
基本读不出来。实测同一张封面图：

| 引擎 | 识别结果 |
| --- | --- |
| 系统自带 OCR | `eevöiUe` / `Recouo` |
| RapidOCR | `eramie Heritage` / `Knows` / `No Borders` |

```bash
python -m pip install -r requirements-rapidocr.lock
```

装完**重启程序**，设置 →「文字识别 → 引擎」里就会多出 RapidOCR。它同时也覆盖了
日语、韩语等系统 OCR 语言包没装的语言。

代价是慢：首次约 2.5 秒（要载模型），之后每次约 1~1.7 秒，而系统 OCR 只要 0.02~0.2 秒。
日常看文档、网页用系统 OCR，碰到封面海报再切过去。

> **RapidOCR 有个坑，程序里已经自动兜住了**：它默认用的是中文识别模型，
> 遇到纯英文的行经常整行不吐空格——`Project or folder` 读成 `Projectorfolder`，
> 拿去翻译就成了「投影文件夹」。程序检测到这种粘成一坨的行，会把整张图交给
> 系统 OCR 重读一次（它对拉丁文分词是准的），按字母序列对上号后把空格补回来。
> 中文行不受影响。代价约 +50~260 毫秒。

> 只有切换到 RapidOCR 时必须重启程序。RapidOCR 依赖的 onnxruntime 只有在 Qt 之前加载才正常，
> 这是它自身的限制，程序启动时会按配置提前加载；其他 OCR 引擎之间切换会立即生效。

## 打包成 exe

```bash
python -m venv .venv-build
.venv-build\Scripts\python.exe -m pip install -r requirements-build.lock
.venv-build\Scripts\python.exe release/check_environment.py --strict-lock
.venv-build\Scripts\python.exe build.py
```

构建环境放在项目内的 `.venv-build`，不会改动系统/全局 Python。后续测试和打包也要
始终显式调用这个目录里的 `python.exe`，避免误用装有其他项目依赖的全局环境。

产物在 `dist/ScreenTranslate/`，分发时整个文件夹拷走，双击里面的 `ScreenTranslate.exe`。
目标机器不需要装 Python。构建脚本会把 `LICENSE.txt`、`THIRD_PARTY_NOTICES.txt` 和
`THIRD_PARTY_LICENSES/` 一并放进该目录；发布 ZIP 和安装器必须保留这些文件。

想要单文件版（分发方便，但每次启动要多等几秒解压）：

```bash
python build.py --onefile
```

单文件构建会另外生成 `dist/ScreenTranslate-licenses/`；对外分发时必须与 EXE 一起提供。

把 RapidOCR 一起打进去（艺术字识别能力见上文；会额外包含 NumPy、OpenCV、
ONNX Runtime 和模型文件，产物会明显增大）：

```bash
python build.py --with-rapidocr
```

## 出问题时的自检

```bash
ScreenTranslate.exe --selftest
```

会逐项检查并弹窗报告：OCR 语言包 → 文字识别 → 译文排版 → 翻译接口 → 全局快捷键，
一眼看出断在哪一环。报告同时写到 `%APPDATA%\ScreenTranslate\selftest.txt`。

从源码运行时是 `python run.pyw --selftest`。

## 自动构建与发布

仓库内置两条 Windows GitHub Actions 工作流：

- `.github/workflows/windows-ci.yml`：在提交和拉取请求上安装并核对锁定依赖、运行回归测试并构建 PyInstaller 文件夹版。
- `.github/workflows/windows-release.yml`：推送 `v1.2.3` 或预发布标签时重复测试和构建，运行离线自检，生成安装器、便携 ZIP 与 `update-manifest.json`，再发布到该仓库的 GitHub Releases。

本项目目前**不使用 Authenticode 代码签名证书**。因此 Windows SmartScreen 可能显示“未知发布者”
或首次下载警告；请只从本项目官方 GitHub Releases 下载，并在运行前核对 Release 来源。

应用内更新只读取当前仓库的 HTTPS `update-manifest.json`，并把安装包来源严格限定为
**同一 owner/repo、版本标签和文件名**。用户点击“下载并安装”后，程序会流式下载到本地更新
缓存，自动核对清单中的文件大小和 SHA-256；校验通过后仍会再次询问，只有用户确认才退出旧版
并启动安装器。程序不会静默安装，也不绕过 Windows 的安全提示；由于当前不使用代码签名证书，
Windows 可能显示“未知发布者”。更新窗口仍可打开 GitHub Release 页面作为手动下载的备用入口。
GitHub 在中国大陆网络中可能访问较慢或暂时不可达，遇到这种情况可稍后重试。完整发布约定见
`docs/releasing.md`。

## 配置文件

`%APPDATA%\ScreenTranslate\config.json`

设置界面改动即时生效并写盘，也可以直接编辑这个文件（改完重启程序）。

**API 密钥用 Windows DPAPI 加密后保存**，盘上长这样：

```json
"key": "dpapi:v1:AQAAANCMnd8BFdERjHoAwE/Cl+sBAAAA..."
```

密钥由你当前的 Windows 账户派生，别的账户、别的机器都解不开，不需要你记任何密码。
这挡住的是最现实的泄漏方式——把 config.json 发给别人、连项目一起传网盘、截图带出去。
它**不能**防住以你的身份运行的程序（同账户下谁都能解），别把它当保险箱。

老的明文配置照样能读，存一次之后自动升级成密文。换了 Windows 账户就解不开了，
界面上会显示成「没填密钥」，重新填一次即可。

## 工作原理

```
快捷键 → 冻结全部屏幕 → 框选
      → 从冻结画面裁剪（不重新截屏，避免把遮罩层截进去）
      → Windows OCR 取文字 + 逐词包围盒
        （包围盒要按 OcrResult.text_angle 转回原图坐标系：
          引擎会先估一个倾斜角把画面摆正再识别，给出的坐标是摆正之后的，
          不转回来译文就整体错位，画面越宽错得越多）
      → 按行距把行聚成段落。分界不是写死的倍数，而是从**这一屏自己的行距分布**里找：
        段内换行和段间空开本来是两簇，用 Otsu 找中间那个谷；分不出两簇才退回按行高判断
      → 再校验「这真的是一段吗」
        （段落除末行外每行都该贴着栏边；列表/菜单不满足，一项一行保持独立。
          还要再排除「被面板边缘切断」的列表：它们右边缘像素级对齐，
          乍看和排满的正文一样，但正文换行是断在词边界上、边缘天然参差）
      → 判断语种，决定译成中文还是英文
      → 调翻译接口（多段一次请求）
      → 采样每段的背景色和文字色，用背景色盖掉原文，
        把译文按自适应字号重排进同一块区域
      → 覆盖窗口精确贴在原位置显示
```

## 代码结构

```
screentrans/
  main.py          托盘、快捷键、整体流程编排
  overlay.py       框选遮罩（每块屏幕一个）
  result.py        译文覆盖窗口
  worker.py        后台线程：识别 + 翻译
  render.py        取色、盖住原文、译文自适应排版
  layout.py        OCR 行 → 段落块
  langdetect.py    语种判定与目标语言决策
  hotkey.py        全局快捷键（Win32 RegisterHotKey）
  capture.py       截屏
  winsys.py        DPI 感知、显示器物理坐标、开机自启
  config.py        配置读写
  ocr/             识别引擎（Windows / Azure / 有道 / RapidOCR）
  translators/     翻译后端（官方 API / 免密网页接口 / AI）
  ui/              设置界面、托盘图标、样式
```

设置界面是左边一列导航 + 右边一页内容：快捷键 / 翻译 / 文字识别 / 显示 / 其他。
改哪一项就立刻存哪一项，没有「保存」按钮。

## 许可证和第三方说明

ScreenTranslate 自有代码采用 [MIT License](LICENSE)。打包程序包含 PySide6/Qt、Pillow、Requests、
Python/WinRT 等第三方组件；可选 RapidOCR 版本还会包含 ONNX Runtime、OpenCV、NumPy 与模型文件。
各组件的许可证、来源和归属见 [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES)。

免密钥翻译和有道云端 OCR 的实现参考了 MIT 许可的 Glance / STranslate 来源链；其中
STranslate 为 Copyright © 2022 zggsong。相关署名和 MIT 条款已保留在第三方声明中。

## 已知限制

- 只支持 Windows（用了 Windows OCR 和 Win32 快捷键 API）
- 系统 OCR 语言包没装的语言识别不了，需按上文安装或改用 RapidOCR
- 盖住原文用的是形态学滤波「抹字」，渐变和纯色背景都能还原；
  背景是复杂照片时抹出来会有点糊
- 竖排文字、艺术字、手写体识别率有限
- 基础版已移除 NumPy/OpenBLAS，并只打包 Windows OCR 实际使用的模块化 WinRT 投影；
  PySide6、Pillow 和 OCR 绑定仍是主要体积。带 RapidOCR 的版本会再包含 OpenCV、
  ONNX Runtime、NumPy 和模型文件，适合确实需要艺术字识别时单独构建
- 划选高亮和底图文字可能差 1~2 像素：显示出来的字是画在图上的，
  可选中的是上面一层透明文字，两套排版引擎有极小差异。复制到的内容始终是准确的
- 译文比原文长很多时（英译中常见），会先往右借空间、右边不够才往下借；
  两边都不够就只能压字号。原文框的宽度只是**原文那几个字**的宽度，
  跟按钮、菜单项的实际宽度没关系，所以借多少是猜的，偶尔会盖到旁边的图标上
- 重新框选仍限制在单块显示器内；拖到屏幕边缘时会停在该显示器边界
