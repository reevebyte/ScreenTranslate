"""翻译后端注册表。新增一个后端只需在这里登记。"""
from __future__ import annotations

from .anthropic import AnthropicTranslator
from .base import TranslateError, Translator, friendly
from .deepl import DeepLTranslator
from .free_web import (
    BingFreeTranslator,
    IcibaFreeTranslator,
    MicrosoftFreeTranslator,
    TencentFreeTranslator,
    YandexFreeTranslator,
)
from .google import GoogleFreeTranslator, GoogleTranslator
from .microsoft import MicrosoftTranslator
from .nvidia import NvidiaTranslator
from .openai_like import OpenAILikeTranslator

PROVIDERS: dict[str, type[Translator]] = {
    cls.name: cls
    for cls in (
        MicrosoftTranslator,
        MicrosoftFreeTranslator,
        GoogleTranslator,
        GoogleFreeTranslator,
        BingFreeTranslator,
        TencentFreeTranslator,
        YandexFreeTranslator,
        IcibaFreeTranslator,
        DeepLTranslator,
        OpenAILikeTranslator,
        NvidiaTranslator,
        AnthropicTranslator,
    )
}

# 设置界面里每个后端要显示哪些输入框：(配置键, 标签, 是否密码, 占位提示)
FIELDS: dict[str, list[tuple[str, str, bool, str]]] = {
    "microsoft": [
        ("key", "密钥 Key", True, "Azure 门户 → Translator 资源 → 密钥"),
        ("region", "区域 Region", False, "例如 eastasia、global"),
        ("endpoint", "终结点", False, "https://api.cognitive.microsofttranslator.com"),
    ],
    "google": [("key", "API Key", True, "Google Cloud Translation API 密钥")],
    "google_free": [],
    "bing_free": [],
    "microsoft_free": [],
    "tencent_free": [],
    "yandex_free": [],
    "iciba_free": [],
    "deepl": [
        ("key", "密钥 Key", True, "免费版密钥以 :fx 结尾"),
    ],
    "openai": [
        ("base_url", "接口地址", False, "https://api.deepseek.com/v1"),
        ("key", "API Key", True, "sk-..."),
        ("model", "模型", False, "deepseek-chat"),
    ],
    "nvidia": [
        ("key", "API Key", True, "nvapi-..."),
        ("model", "模型", False, "点右边「刷新」拉取可用模型"),
        ("base_url", "接口地址", False, "https://integrate.api.nvidia.com/v1"),
    ],
    "anthropic": [
        ("key", "API Key", True, "sk-ant-..."),
        ("model", "模型", False, "claude-sonnet-5"),
        ("base_url", "接口地址", False, "https://api.anthropic.com"),
    ],
}

HINTS: dict[str, str] = {
    "microsoft": "免费层每月 200 万字符，国内可直连。",
    "google": "需要 Google Cloud 项目并启用 Translation API。",
    "google_free": "无需密钥，非官方接口，有频率限制；国内通常需要代理。",
    "bing_free": "无需密钥，使用必应网页接口；可能限流或随网页改版失效。",
    "microsoft_free": "无需密钥，模拟微软客户端签名；不是 Azure 正式开发者 API。",
    "tencent_free": "无需密钥，使用腾讯交互翻译网页接口；可能限流或改版。",
    "yandex_free": "无需密钥，使用 Yandex 移动端接口；国内网络可用性不固定。",
    "iciba_free": "无需密钥，使用词霸网页接口；适合中英文短文本。",
    "deepl": "译文质量高，免费版每月 50 万字符。",
    "openai": "填 base_url + key + model 即可对接 DeepSeek / 智谱 / 通义 / 本地 Ollama 等。",
    "nvidia": "到 build.nvidia.com 领取 nvapi- 开头的 Key，地址已预置，新账号有免费额度。",
    "anthropic": "使用 Claude 的 Messages API。",
}


def build(provider: str, opts: dict) -> Translator:
    cls = PROVIDERS.get(provider)
    if cls is None:
        raise TranslateError(f"未知的翻译引擎：{provider}")
    return cls(opts)


__all__ = ["PROVIDERS", "FIELDS", "HINTS", "build", "Translator",
           "TranslateError", "friendly"]
