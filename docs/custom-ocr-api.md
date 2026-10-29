# 自定义 OCR 接口

在“服务中心 → 文字识别 → 自定义 OCR”选择协议，填写地址，按“使用此服务”启用。仅展开表单不会切换当前引擎。密钥可选，填写后以 `Authorization: Bearer <key>` 发送，沿用系统凭据库；凭据库不可用时仅保留在当前会话，不写明文配置。

地址使用 HTTPS；本机服务允许 HTTP。配置校验不允许 URL 中携带用户名、密码、查询参数或 fragment。

## OpenAI 兼容视觉模型

填写基础地址（例如 `http://localhost:11434/v1`）与支持视觉输入的模型。应用调用 `/chat/completions`；也可以填写完整的 `/chat/completions` 地址，不会重复拼接。

请求采用 user 消息中的 text 和 image_url 内容，图像为 `data:image/png;base64,...`。提示模型只返回原文和坐标，不翻译。应用接受标准 `choices[0].message.content` 中的 JSON，允许外层 Markdown JSON 代码围栏。此协议不调用 Responses API。

## HTTP / JSON

应用直接向填写的完整地址 POST：

```json
{
  "image": "<PNG Base64，不含 data: 前缀>",
  "format": "png",
  "width": 1200,
  "height": 800,
  "languages": ["zh-Hans-CN", "en-US"]
}
```

响应示例：

```json
{
  "blocks": [
    {
      "text": "Read the world.",
      "bounds": { "x": 40, "y": 60, "width": 300, "height": 32 },
      "confidence": 0.98
    }
  ]
}
```

`lines` 可替代 `blocks`。坐标必须是本次请求图片中的物理像素、位于图像范围内，宽高大于 0。`confidence` 可省略。开启小图放大后，以请求的 width/height 为准；应用将坐标换回原始截图尺寸。

也接受 `{"text":"完整识别文本"}`。这种响应成为覆盖整幅图像的一个文字块，适合提取文字；精确原位翻译需要接口提供真实文字块坐标，应用不会凭空猜测各行位置。

## 限制与错误

PNG 请求最大 32 MiB，JSON 响应最大 4 MiB，最多 4096 块、总文字最多 100,000 UTF-8 字节。网络请求上限 60 秒，识别流水线外层上限 90 秒，可取消。错误、鉴权失败、限流、无效 JSON 和无效坐标会返回可恢复错误，并引导到文字识别配置。

应用不会记录截图、识别内容或凭据。选区图片会发送到填写的接口；本地服务和远程服务均由用户自行配置。
