"""Azure AI Vision Read v3.2 OCR。"""
from __future__ import annotations

import io
import time

import requests
from PIL import Image

from ..network import (
    ensure_success,
    json_response,
    redact_sensitive,
    validate_api_base_url,
    validate_same_origin_https,
)
from .base import Line, OcrError, Word

REQUEST_TIMEOUT = 20
POLL_TIMEOUT = 20
POLL_INTERVAL = 0.35


def _png_bytes(image: Image.Image) -> bytes:
    output = io.BytesIO()
    image.save(output, format="PNG")
    return output.getvalue()


def _rect(polygon) -> tuple[float, float, float, float]:
    if not isinstance(polygon, list) or len(polygon) < 8 or len(polygon) % 2:
        raise OcrError("Azure OCR 返回了无效坐标")
    try:
        xs = [float(value) for value in polygon[0::2]]
        ys = [float(value) for value in polygon[1::2]]
    except (TypeError, ValueError) as exc:
        raise OcrError("Azure OCR 返回了无效坐标") from exc
    return min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys)


def _confidence(words: list[Word]) -> float | None:
    known = [
        (word.confidence, max(1, sum(not char.isspace() for char in word.text)))
        for word in words
        if word.confidence is not None
    ]
    if not known:
        return None
    weight = sum(size for _value, size in known)
    return sum(value * size for value, size in known) / weight


def _parse(payload) -> list[Line]:
    try:
        pages = payload["analyzeResult"]["readResults"]
    except (KeyError, TypeError) as exc:
        raise OcrError("Azure OCR 返回格式已经变化") from exc

    output = []
    for page in pages:
        for raw_line in page.get("lines", []):
            text = str(raw_line.get("text") or "").strip()
            if not text:
                continue
            words = []
            for raw_word in raw_line.get("words", []):
                word_text = str(raw_word.get("text") or "").strip()
                if not word_text:
                    continue
                x, y, width, height = _rect(raw_word.get("boundingBox"))
                raw_confidence = raw_word.get("confidence")
                confidence = (
                    float(raw_confidence)
                    if isinstance(raw_confidence, (int, float))
                    and 0 <= float(raw_confidence) <= 1
                    else None
                )
                words.append(Word(word_text, x, y, width, height, confidence))
            x, y, width, height = _rect(raw_line.get("boundingBox"))
            output.append(Line(text, x, y, width, height, words, _confidence(words)))
    return output


def recognize(
    image: Image.Image,
    opts: dict,
    *,
    session: requests.Session | None = None,
) -> list[Line]:
    endpoint = str((opts or {}).get("endpoint") or "").strip()
    key = str((opts or {}).get("key") or "").strip()
    if not endpoint:
        raise OcrError("尚未填写 Azure Vision Endpoint（设置 → 文字识别）")
    endpoint = validate_api_base_url(
        endpoint,
        label="Azure Vision Endpoint",
        error_type=OcrError,
    )
    if not key:
        raise OcrError("尚未填写 Azure Vision Key（设置 → 文字识别）")

    owned_session = session is None
    http = session or requests.Session()
    headers = {"Ocp-Apim-Subscription-Key": key}
    try:
        response = http.post(
            f"{endpoint}/vision/v3.2/read/analyze",
            params={"readingOrder": "natural", "model-version": "latest"},
            headers={**headers, "Content-Type": "application/octet-stream"},
            data=_png_bytes(image),
            timeout=REQUEST_TIMEOUT,
            allow_redirects=False,
            stream=True,
        )
        try:
            ensure_success(
                response,
                label="Azure OCR",
                error_type=OcrError,
                secrets=[key],
            )
            operation_url = response.headers.get("Operation-Location")
            if not operation_url:
                raise OcrError("Azure OCR 没有返回 Operation-Location")
        finally:
            response.close()

        operation_url = validate_same_origin_https(
            operation_url,
            endpoint,
            label="Azure OCR Operation-Location",
            error_type=OcrError,
        )

        deadline = time.monotonic() + POLL_TIMEOUT
        while True:
            result = http.get(
                operation_url,
                headers=headers,
                timeout=REQUEST_TIMEOUT,
                allow_redirects=False,
                stream=True,
            )
            ensure_success(
                result,
                label="Azure OCR",
                error_type=OcrError,
                secrets=[key],
            )
            payload = json_response(result, label="Azure OCR", error_type=OcrError)
            status = str(payload.get("status") or "").lower()
            if status == "succeeded":
                return _parse(payload)
            if status == "failed":
                detail = redact_sensitive(
                    payload.get("error") or "未知错误",
                    [key],
                )[:300]
                raise OcrError(f"Azure OCR 识别失败：{detail}")
            if status not in ("notstarted", "running"):
                raise OcrError(f"Azure OCR 返回了未知状态：{status or '空'}")
            if time.monotonic() >= deadline:
                raise OcrError(f"Azure OCR 等待结果超时（{POLL_TIMEOUT} 秒）")
            time.sleep(POLL_INTERVAL)
    except requests.RequestException as exc:
        detail = redact_sensitive(exc, [key])
        raise OcrError(f"Azure OCR 请求失败：{detail}") from exc
    finally:
        if owned_session:
            http.close()


__all__ = ["recognize"]
