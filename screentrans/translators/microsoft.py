"""微软 Azure 翻译（Translator v3）。

免费层每月 200 万字符，国内直连可用。
申请：Azure 门户 → 创建资源 → Translator → 密钥和终结点。
"""
from __future__ import annotations

from ..network import validate_api_base_url
from .base import Translator, TranslateError, TIMEOUT

_CODES = {"zh-Hans": "zh-Hans", "zh-Hant": "zh-Hant", "en": "en"}


class MicrosoftTranslator(Translator):
    name = "microsoft"
    label = "微软 Azure 翻译"

    max_items = 100
    max_chars = 40000

    def _translate_batch(self, texts, target, source):
        key = (self.opts.get("key") or "").strip()
        if not key:
            raise TranslateError("尚未填写 Azure 翻译密钥（设置 → 翻译引擎）")
        endpoint = validate_api_base_url(
            self.opts.get("endpoint") or "https://api.cognitive.microsofttranslator.com",
            label="Azure 翻译 Endpoint",
            error_type=TranslateError,
        )
        region = (self.opts.get("region") or "").strip()

        params = {"api-version": "3.0", "to": _CODES.get(target, target), "textType": "plain"}
        if source and source not in ("auto", "other"):
            params["from"] = source
        headers = {
            "Ocp-Apim-Subscription-Key": key,
            "Content-Type": "application/json; charset=utf-8",
        }
        if region:
            headers["Ocp-Apim-Subscription-Region"] = region

        resp = self._session().post(
            f"{endpoint}/translate",
            params=params,
            headers=headers,
            json=[{"Text": t} for t in texts],
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(resp, "Azure 翻译")
        data = self._json(resp, "Azure 翻译")
        if isinstance(data, dict) and "error" in data:
            raise TranslateError(data["error"].get("message", str(data)))
        return [item["translations"][0]["text"] for item in data]
