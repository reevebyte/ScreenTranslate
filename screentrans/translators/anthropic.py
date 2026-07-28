"""Anthropic Claude 原生接口（Messages API）。"""
from __future__ import annotations

from ..network import validate_api_base_url
from . import _ai
from .base import Translator, TranslateError, TIMEOUT


class AnthropicTranslator(Translator):
    name = "anthropic"
    label = "Claude（Anthropic）"

    max_items = 40
    max_chars = 6000
    supports_model_list = True

    def _base_and_headers(self) -> tuple[str, dict]:
        key = (self.opts.get("key") or "").strip()
        if not key:
            raise TranslateError("尚未填写 Anthropic API Key")
        base = validate_api_base_url(
            self.opts.get("base_url") or "https://api.anthropic.com",
            label="Anthropic 接口地址",
            error_type=TranslateError,
        )
        return base, {
            "x-api-key": key,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json",
        }

    def list_models(self) -> list[str]:
        base, headers = self._base_and_headers()
        resp = self._session().get(
            f"{base}/v1/models",
            params={"limit": 100},
            headers=headers,
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(resp, "Anthropic 模型列表")
        data = self._json(resp, "Anthropic 模型列表")
        names = [m.get("id") for m in data.get("data", []) if m.get("id")]
        if not names:
            raise TranslateError("接口没有返回任何模型")
        return names

    def _call(self, system: str, user: str) -> str:
        self._raise_if_cancelled()
        base, headers = self._base_and_headers()
        model = (self.opts.get("model") or "claude-sonnet-5").strip()

        resp = self._session().post(
            f"{base}/v1/messages",
            headers=headers,
            json={
                "model": model,
                "max_tokens": 4096,
                "temperature": 0,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            },
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(resp, "Anthropic")
        data = self._json(resp, "Anthropic")
        try:
            return "".join(
                blk.get("text", "") for blk in data["content"] if blk.get("type") == "text"
            )
        except (KeyError, TypeError) as exc:
            raise TranslateError("接口返回格式异常") from exc

    def _translate_batch(self, texts, target, source):
        return _ai.batched(texts, self._call, target)
