"""有道云端图片 OCR。

复用有道桌面词典的图片翻译网页接口，只读取响应里的原文和位置。该接口
不需要用户密钥，但不是正式开发者 API，可能限流或随服务端改版失效。
"""
from __future__ import annotations

import base64
import hashlib
import io
import uuid

import requests
from PIL import Image

from ..network import ensure_success, json_response, redact_sensitive
from .base import Line, OcrError

ENDPOINT = "https://ocrtran.youdao.com/ocr/imgtranocr"
TIMEOUT = 20
_CLIENTELE = "deskdict"
_IMAGE_TRANSLATE_SECRET = "VPaHE3kX_vl4BhgYiu2n"
_USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/127.0.0.0 Safari/537.36"
)


def _png_bytes(image: Image.Image) -> bytes:
    output = io.BytesIO()
    image.save(output, format="PNG")
    return output.getvalue()


def _salt() -> str:
    value = uuid.uuid4().int % 1_000_000_000_000_000_000
    return f"{value / 1_000_000_000_000_000_000:.18f}".rstrip("0")


def _sign(image_bytes: bytes, salt: str) -> str:
    encoded = base64.b64encode(image_bytes).decode("ascii")
    digest_source = f"{encoded[:10]}{len(encoded)}{encoded[-10:]}"
    raw = f"{_CLIENTELE}{digest_source}{salt}{_IMAGE_TRANSLATE_SECRET}"
    return hashlib.md5(raw.encode()).hexdigest()


def _bounds(raw) -> tuple[float, float, float, float]:
    try:
        values = tuple(float(item.strip()) for item in str(raw).split(","))
    except ValueError as exc:
        raise OcrError(f"云端 OCR 返回了无效坐标：{raw}") from exc
    if len(values) != 4 or values[2] <= 0 or values[3] <= 0:
        raise OcrError(f"云端 OCR 返回了无效坐标：{raw}")
    return values


def _parse(payload) -> list[Line]:
    if not isinstance(payload, dict):
        raise OcrError("云端 OCR 返回格式已经变化")
    if str(payload.get("errorCode", "")) != "0":
        raise OcrError(f"云端 OCR 返回错误：{payload.get('errorCode', '未知')}")

    output = []
    for region in payload.get("resRegions", []):
        if not isinstance(region, dict):
            continue
        x, y, width, height = _bounds(region.get("boundingBox", ""))

        # 有道的 boundingBox 是整段区域，不是单行框。优先使用响应里的
        # lines 拆成视觉行，否则渲染器会把整段高度误当成单行字号。
        texts = [
            str(item.get("text") or "").strip()
            for item in region.get("lines", [])
            if isinstance(item, dict) and str(item.get("text") or "").strip()
        ]
        if not texts:
            context = str(region.get("context") or "").strip()
            texts = [line.strip() for line in context.splitlines() if line.strip()]
        if not texts:
            continue

        line_height = height / len(texts)
        for index, text in enumerate(texts):
            output.append(
                Line(
                    text,
                    x,
                    y + line_height * index,
                    width,
                    line_height,
                )
            )
    return output


def recognize(image: Image.Image, *, session: requests.Session | None = None) -> list[Line]:
    image_bytes = _png_bytes(image)
    salt = _salt()
    owned_session = session is None
    http = session or requests.Session()
    try:
        response = http.post(
            ENDPOINT,
            headers={"User-Agent": _USER_AGENT, "Accept": "*/*"},
            files={"multipartFile": ("capture.png", image_bytes, "image/png")},
            data={
                "clientele": _CLIENTELE,
                "salt": salt,
                "sign": _sign(image_bytes, salt),
                "from": "auto",
                "to": "zh-CHS",
                "isSaveHistory": "false",
                "isSyncSaveHistory": "false",
                "funDesc": "photo_translate",
            },
            timeout=TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        ensure_success(response, label="云端 OCR", error_type=OcrError)
        payload = json_response(response, label="云端 OCR", error_type=OcrError)
        return _parse(payload)
    except requests.RequestException as exc:
        raise OcrError(f"云端 OCR 请求失败：{redact_sensitive(exc)}") from exc
    finally:
        if owned_session:
            http.close()


__all__ = ["recognize"]
