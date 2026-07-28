"""任何 OpenAI 兼容接口（/v1/chat/completions）。

一个配置项覆盖大部分主流服务，把 base_url / model 换掉即可：
  DeepSeek     https://api.deepseek.com/v1              deepseek-chat
  智谱 GLM     https://open.bigmodel.cn/api/paas/v4     glm-4-flash
  通义千问     https://dashscope.aliyuncs.com/compatible-mode/v1   qwen-plus
  月之暗面     https://api.moonshot.cn/v1               moonshot-v1-8k
  硅基流动     https://api.siliconflow.cn/v1            Qwen/Qwen2.5-7B-Instruct
  OpenAI       https://api.openai.com/v1                gpt-4o-mini
  本地 Ollama  http://localhost:11434/v1                qwen2.5:7b
"""
from __future__ import annotations

from ..network import validate_api_base_url
from . import _ai
from .base import Translator, TranslateError, TIMEOUT


class OpenAILikeTranslator(Translator):
    name = "openai"
    label = "AI 大模型（OpenAI 兼容）"

    max_items = 40          # 段数太多模型容易漏条目
    max_chars = 6000
    supports_model_list = True

    # 子类（例如英伟达）可以在这里预置好地址和模型，用户只要填 Key
    default_base_url = ""
    default_model = ""

    def _root(self) -> str:
        base = (self.opts.get("base_url") or self.default_base_url).strip().rstrip("/")
        if not base:
            raise TranslateError("尚未填写接口地址 base_url")
        # 用户可能把完整的 /chat/completions 也粘进来了，统一还原成根地址
        for suffix in ("/chat/completions", "/completions"):
            if base.endswith(suffix):
                base = base[: -len(suffix)]
        key = (self.opts.get("key") or "").strip()
        return validate_api_base_url(
            base,
            label="OpenAI 兼容接口地址",
            error_type=TranslateError,
            key_present=bool(key),
            allow_keyless_loopback_http=True,
        )

    def _headers(self) -> dict:
        headers = {"Content-Type": "application/json"}
        key = (self.opts.get("key") or "").strip()
        if key:
            headers["Authorization"] = f"Bearer {key}"
        return headers

    def list_models(self) -> list[str]:
        resp = self._session().get(
            f"{self._root()}/models",
            headers=self._headers(),
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(resp, "模型列表")
        data = self._json(resp, "模型列表")
        items = data.get("data") if isinstance(data, dict) else data
        if not isinstance(items, list):
            raise TranslateError("模型列表返回格式异常")
        names = [m.get("id") for m in items if isinstance(m, dict) and m.get("id")]
        if not names:
            raise TranslateError("接口没有返回任何模型")
        return sorted(names)

    def _call(self, system: str, user: str) -> str:
        # batched() 可能在协议失败后逐段回退；窗口已关闭时不能继续发几十个请求。
        self._raise_if_cancelled()
        model = (self.opts.get("model") or self.default_model).strip()
        if not model:
            raise TranslateError("尚未选择模型，点模型栏右边的「刷新」拉取可用列表")

        resp = self._session().post(
            f"{self._root()}/chat/completions",
            headers=self._headers(),
            json={
                "model": model,
                "temperature": 0,
                "stream": False,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            },
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(resp, "OpenAI 兼容接口")
        data = self._json(resp, "OpenAI 兼容接口")
        try:
            return data["choices"][0]["message"]["content"] or ""
        except (KeyError, IndexError, TypeError) as exc:
            raise TranslateError("接口返回格式异常") from exc

    def _translate_batch(self, texts, target, source):
        return _ai.batched(texts, self._call, target)
