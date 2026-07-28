"""DeepL 翻译。免费版密钥以 :fx 结尾，走 api-free.deepl.com。"""
from __future__ import annotations

from .base import Translator, TranslateError, TIMEOUT

_CODES = {"zh-Hans": "ZH", "zh-Hant": "ZH", "en": "EN-US"}


class DeepLTranslator(Translator):
    name = "deepl"
    label = "DeepL"

    max_items = 50          # DeepL 单次最多 50 个 text 参数
    max_chars = 30000

    def _translate_batch(self, texts, target, source):
        key = (self.opts.get("key") or "").strip()
        if not key:
            raise TranslateError("尚未填写 DeepL 密钥")
        free = self.opts.get("free_plan", True) or key.endswith(":fx")
        host = "https://api-free.deepl.com" if free else "https://api.deepl.com"

        payload = {"text": texts, "target_lang": _CODES.get(target, target.upper())}
        if source and source not in ("auto", "other"):
            payload["source_lang"] = _CODES.get(source, source.upper()).split("-")[0]

        resp = self._session().post(
            f"{host}/v2/translate",
            headers={"Authorization": f"DeepL-Auth-Key {key}"},
            json=payload,
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(resp, "DeepL")
        items = self._json(resp, "DeepL").get("translations", [])
        if len(items) != len(texts):
            raise TranslateError(f"返回条数不匹配：{len(items)} != {len(texts)}")
        return [it["text"] for it in items]
