"""英伟达 NIM（build.nvidia.com）。

接口是 OpenAI 兼容的，所以直接复用 OpenAI 那套实现，只是预置好地址，
用户只需要填一个 Key（形如 nvapi-xxxx）。

拿 Key：https://build.nvidia.com → 挑一个模型 → Get API Key。
新账号有免费额度。

常用模型（填在「模型」一栏）：
    qwen/qwen2.5-7b-instruct            快，中英互译够用
    meta/llama-3.3-70b-instruct         综合质量好
    deepseek-ai/deepseek-v3             中文强
    microsoft/phi-4-mini-instruct       轻量
"""
from __future__ import annotations

from .openai_like import OpenAILikeTranslator


class NvidiaTranslator(OpenAILikeTranslator):
    name = "nvidia"
    label = "英伟达 NIM"

    default_base_url = "https://integrate.api.nvidia.com/v1"
    # 不预设模型：NVIDIA 目录会变，写死一个不存在的名字只会回 404，
    # 让用户点「刷新」从 /v1/models 拉真实列表更靠谱。
    default_model = ""
