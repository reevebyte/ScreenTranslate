"""免密钥网页翻译接口。

这些接口模拟网页或官方客户端请求，不属于面向开发者的正式 API。它们适合
个人轻量使用，但可能限流或随服务端改版失效。
"""
from __future__ import annotations

import base64
import hashlib
import hmac
import re
import uuid
from datetime import datetime, timezone
from urllib.parse import quote

from ..network import json_response, text_response
from .base import TIMEOUT, TranslateError, Translator

BROWSER_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/127.0.0.0 Safari/537.36"
)


def _json(response, engine: str):
    return json_response(response, label=engine, error_type=TranslateError)


def _translated(value, engine: str) -> str:
    text = value if isinstance(value, str) else ""
    if not text.strip():
        raise TranslateError(f"{engine} 返回了空译文")
    return text


def _source(code: str | None) -> str:
    return code or "auto"


class _OneByOneFreeTranslator(Translator):
    needs_key = False
    max_items = 20
    max_chars = 8000

    def _translate_batch(self, texts, target, source):
        output = []
        for text in texts:
            self._raise_if_cancelled()
            output.append(self._translate_one(text, target, source))
        return output

    def _translate_one(self, text: str, target: str, source: str | None) -> str:
        raise NotImplementedError


class BingFreeTranslator(_OneByOneFreeTranslator):
    name = "bing_free"
    label = "必应翻译（免密钥）"

    _CN_HOST = "https://cn.bing.com"
    _WWW_HOST = "https://www.bing.com"

    def __init__(self, opts: dict):
        super().__init__(opts)
        self._token: tuple[str, str, str, str, str] | None = None

    def _fetch_token(self, host: str) -> tuple[str, str, str, str, str]:
        response = self._session().get(
            f"{host}/translator",
            headers={"User-Agent": BROWSER_UA},
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(response, "必应翻译")
        html = text_response(response, label="必应翻译", error_type=TranslateError)

        token_match = re.search(
            r'params_AbusePreventionHelper\s*=\s*\[\s*([^,\]]+)\s*,\s*"([^"]+)"',
            html,
        )
        ig_match = re.search(r'IG:"([^"]+)"', html)
        if token_match is None or ig_match is None:
            raise TranslateError("必应网页令牌格式已经变化")

        iid_match = re.search(r'data-iid="([^"]+)"', html)
        key = token_match.group(1).strip().strip('"')
        token = token_match.group(2)
        iid = iid_match.group(1) if iid_match else "translator.5023"
        return token, key, ig_match.group(1), iid, host

    def _get_token(self):
        if self._token is not None:
            return self._token
        try:
            self._token = self._fetch_token(self._CN_HOST)
        except Exception:
            self._token = self._fetch_token(self._WWW_HOST)
        return self._token

    @staticmethod
    def _lang(code: str | None) -> str:
        value = _source(code)
        return "auto-detect" if value == "auto" else value

    def _translate_one(self, text: str, target: str, source: str | None) -> str:
        token, key, ig, iid, host = self._get_token()
        response = self._session().post(
            f"{host}/ttranslatev3",
            params={"isVertical": "1", "IG": ig, "IID": iid},
            headers={"Referer": f"{host}/translator", "Origin": host},
            data={
                "fromLang": self._lang(source),
                "to": self._lang(target),
                "text": text,
                "token": token,
                "key": key,
            },
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        if response.status_code in (401, 403, 429):
            self._token = None
        self._check(response, "必应翻译")
        body = _json(response, "必应翻译")
        try:
            translated = body[0]["translations"][0]["text"]
        except (KeyError, IndexError, TypeError) as exc:
            self._token = None
            raise TranslateError("必应翻译返回格式已经变化") from exc
        return _translated(translated, "必应翻译")


_MS_PRIVATE_KEY = bytes(
    [
        0xA2, 0x29, 0x3A, 0x3D, 0xD0, 0xDD, 0x32, 0x73, 0x97, 0x7A, 0x64, 0xDB,
        0xC2, 0xF3, 0x27, 0xF5, 0xD7, 0xBF, 0x87, 0xD9, 0x45, 0x9D, 0xF0, 0x5A,
        0x09, 0x66, 0xC6, 0x30, 0xC6, 0x6A, 0xAA, 0x84, 0x9A, 0x41, 0xAA, 0x94,
        0x3A, 0xA8, 0xD5, 0x1A, 0x6E, 0x4D, 0xAA, 0xC9, 0xA3, 0x70, 0x12, 0x35,
        0xC7, 0xEB, 0x12, 0xF6, 0xE8, 0x23, 0x07, 0x9E, 0x47, 0x10, 0x95, 0x91,
        0x88, 0x55, 0xD8, 0x17,
    ]
)
_MS_ENDPOINT = "api.cognitive.microsofttranslator.com"
_WEEKDAYS = ("Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun")
_MONTHS = ("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")


def _microsoft_signature(request_path: str) -> str:
    now = datetime.now(timezone.utc)
    date = (
        f"{_WEEKDAYS[now.weekday()]}, {now.day:02d} {_MONTHS[now.month - 1]} "
        f"{now.year:04d} {now:%H:%M:%S}GMT"
    )
    guid = uuid.uuid4().hex
    escaped = quote(request_path, safe="-_.~")
    raw = f"MSTranslatorAndroidApp{escaped}{date}{guid}".lower().encode()
    digest = hmac.new(_MS_PRIVATE_KEY, raw, hashlib.sha256).digest()
    encoded = base64.b64encode(digest).decode("ascii")
    return f"MSTranslatorAndroidApp::{encoded}::{date}::{guid}"


class MicrosoftFreeTranslator(_OneByOneFreeTranslator):
    name = "microsoft_free"
    label = "微软翻译（免密钥）"

    def _translate_one(self, text: str, target: str, source: str | None) -> str:
        request_path = f"{_MS_ENDPOINT}/translate?api-version=3.0&to={target}"
        if source and source != "auto":
            request_path += f"&from={source}"
        response = self._session().post(
            f"https://{request_path}",
            headers={
                "X-MT-Signature": _microsoft_signature(request_path),
                "User-Agent": BROWSER_UA,
            },
            json=[{"Text": text}],
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(response, "微软免密翻译")
        body = _json(response, "微软免密翻译")
        try:
            translated = body[0]["translations"][0]["text"]
        except (KeyError, IndexError, TypeError) as exc:
            raise TranslateError("微软免密翻译返回格式已经变化") from exc
        return _translated(translated, "微软免密翻译")


class TencentFreeTranslator(_OneByOneFreeTranslator):
    name = "tencent_free"
    label = "腾讯交互翻译（免密钥）"

    @staticmethod
    def _lang(code: str | None) -> str:
        return {"zh-Hans": "zh", "zh-Hant": "zh-TW"}.get(_source(code), _source(code))

    def _translate_one(self, text: str, target: str, source: str | None) -> str:
        payload = {
            "header": {
                "fn": "auto_translation_block",
                "client_key": (
                    "browser-chrome-110.0.0-Mac OS-df4bd4c5-"
                    "a65d-44b2-a40f-42f34f3535f2-1677486696487"
                ),
            },
            "type": "plain",
            "model_category": "normal",
            "source": {"lang": self._lang(source), "text_block": text},
            "target": {"lang": self._lang(target)},
        }
        response = self._session().post(
            "https://transmart.qq.com/api/imt",
            headers={"User-Agent": BROWSER_UA, "Referer": "https://yi.qq.com/zh-CN/index"},
            json=payload,
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(response, "腾讯交互翻译")
        translated = _json(response, "腾讯交互翻译").get("auto_translation")
        if isinstance(translated, list):
            translated = "\n".join(item for item in translated if isinstance(item, str))
        return _translated(translated, "腾讯交互翻译")


class YandexFreeTranslator(_OneByOneFreeTranslator):
    name = "yandex_free"
    label = "Yandex 翻译（免密钥）"

    @staticmethod
    def _lang(code: str | None) -> str:
        value = _source(code)
        return "zh" if value in ("zh-Hans", "zh-Hant") else value

    def _translate_one(self, text: str, target: str, source: str | None) -> str:
        source_code = self._lang(source)
        target_code = self._lang(target)
        language = target_code if source_code == "auto" else f"{source_code}-{target_code}"
        response = self._session().post(
            "https://translate.yandex.net/api/v1/tr.json/translate",
            params={"ucid": uuid.uuid4().hex, "srv": "android", "format": "text"},
            headers={"User-Agent": "ru.yandex.translate/3.20.2024"},
            data={"text": text, "lang": language},
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(response, "Yandex 翻译")
        body = _json(response, "Yandex 翻译")
        translated = body.get("text", [""])
        return _translated(translated[0] if translated else "", "Yandex 翻译")


_ICIBA_PATH = "/dictionary/fy/batch"
_ICIBA_CLIENT = "6"
_ICIBA_KEY = "1000006"
_ICIBA_SALT = "7ece94d9f9c202b0d2ec557dg4r9bc"


class IcibaFreeTranslator(Translator):
    name = "iciba_free"
    label = "词霸翻译（免密钥）"
    needs_key = False
    max_items = 50
    max_chars = 12000

    @staticmethod
    def _lang(code: str | None) -> str:
        return {"zh-Hans": "zh", "zh-Hant": "cht"}.get(_source(code), _source(code))

    def _translate_batch(self, texts, target, source):
        timestamp = str(int(datetime.now(timezone.utc).timestamp() * 1000))
        sign_source = f"{_ICIBA_PATH}{_ICIBA_CLIENT}{_ICIBA_KEY}{timestamp}{_ICIBA_SALT}"
        signature = hashlib.md5(sign_source.encode()).hexdigest()
        response = self._session().post(
            f"https://dictionary.iciba.com{_ICIBA_PATH}",
            params={
                "client": _ICIBA_CLIENT,
                "key": _ICIBA_KEY,
                "timestamp": timestamp,
                "signature": signature,
            },
            headers={
                "Origin": "https://www.iciba.com",
                "Referer": "https://www.iciba.com/",
                "User-Agent": BROWSER_UA,
            },
            json={
                "from": self._lang(source),
                "to": self._lang(target),
                "textList": texts,
            },
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        self._check(response, "词霸翻译")
        body = _json(response, "词霸翻译")
        if str(body.get("code")) != "1":
            raise TranslateError("词霸翻译返回了错误状态")
        output = []
        for item in body.get("data", []):
            value = item.get("out", "") if isinstance(item, dict) else item
            output.append(_translated(value, "词霸翻译"))
        return output
