"""谷歌翻译。两种模式：

  google       官方 Cloud Translation v2，需要 API Key，稳定。
  google_free  网页版公开接口，不需要任何密钥，但是非官方、有频率限制，
               且 translate.googleapis.com 在部分网络环境下需要代理才能访问。
"""
from __future__ import annotations

from .base import Translator, TranslateError, TIMEOUT

_CODES = {"zh-Hans": "zh-CN", "zh-Hant": "zh-TW", "en": "en"}


class GoogleTranslator(Translator):
    name = "google"
    label = "谷歌翻译（官方 API）"
    max_items = 100
    max_chars = 30000

    def _translate_batch(self, texts, target, source):
        key = (self.opts.get("key") or "").strip()
        if not key:
            raise TranslateError("尚未填写 Google Cloud Translation API Key")
        payload = {"q": texts, "target": _CODES.get(target, target), "format": "text"}
        if source and source not in ("auto", "other"):
            payload["source"] = _CODES.get(source, source)
        resp = self._session().post(
            "https://translation.googleapis.com/language/translate/v2",
            headers={"X-Goog-Api-Key": key},
            json=payload,
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(resp, "Google 翻译")
        data = self._json(resp, "Google 翻译")
        items = data.get("data", {}).get("translations", [])
        if len(items) != len(texts):
            raise TranslateError(f"返回条数不匹配：{len(items)} != {len(texts)}")
        return [_unescape(it["translatedText"]) for it in items]


class GoogleFreeTranslator(Translator):
    name = "google_free"
    label = "谷歌翻译（免密钥）"
    needs_key = False
    max_items = 20
    max_chars = 8000

    def _translate_batch(self, texts, target, source):
        sess = self._session()
        out = []
        for text in texts:
            self._raise_if_cancelled()
            resp = sess.get(
                "https://translate.googleapis.com/translate_a/single",
                params={
                    "client": "gtx",
                    "sl": _CODES.get(source, source) if source and source != "other" else "auto",
                    "tl": _CODES.get(target, target),
                    "dt": "t",
                    "q": text,
                },
                timeout=TIMEOUT,
                allow_redirects=False,
                stream=True,
            )
            self._check(resp, "Google 免密翻译")
            try:
                data = self._json(resp, "Google 免密翻译")
                out.append("".join(seg[0] for seg in data[0] if seg and seg[0]))
            except Exception as exc:
                raise TranslateError(f"无法解析谷歌返回内容：{exc}") from exc
        return out


def _unescape(s: str) -> str:
    import html

    return html.unescape(s)
