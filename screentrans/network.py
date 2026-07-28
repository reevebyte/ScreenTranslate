"""Small, shared network-safety helpers for cloud backends."""
from __future__ import annotations

import ipaddress
import json
import re
from collections.abc import Mapping, Sequence
from urllib.parse import quote, quote_plus, urlsplit

import requests

MAX_JSON_BYTES = 4 * 1024 * 1024
MAX_TEXT_BYTES = 2 * 1024 * 1024
MAX_ERROR_BYTES = 32 * 1024

_SENSITIVE_QUERY = re.compile(
    r"(?i)([?&](?:api[-_]?key|key|token|access[-_]?token|signature|sig)=)([^&#\s]*)"
)
_SENSITIVE_HEADER = re.compile(
    r"(?i)((?:authorization|x-goog-api-key|x-api-key|api-key|"
    r"ocp-apim-subscription-key)\s*[:=]\s*(?:bearer\s+)?)(['\"]?)[^,'\"\s}]+"
)
_KNOWN_KEY_SHAPE = re.compile(r"\b(?:sk-(?:ant-)?|nvapi-)[A-Za-z0-9._-]{6,}")


def configured_secrets(options: Mapping | None) -> list[str]:
    """Return only values stored under secret-like option names."""
    if not isinstance(options, Mapping):
        return []
    return [
        str(value).strip()
        for name, value in options.items()
        if str(name).lower() in {"key", "api_key", "apikey", "token"}
        and str(value or "").strip()
    ]


def redact_sensitive(value: object, secrets: Sequence[str] = ()) -> str:
    """Remove credentials from exception text before it reaches logs or UI."""
    text = str(value)
    for secret in secrets:
        secret = str(secret or "")
        # Replacing very short strings would make the rest of the message unreadable.
        if len(secret) < 4 and not any(ord(char) < 32 for char in secret):
            continue
        variants = {
            secret,
            quote(secret, safe=""),
            quote_plus(secret, safe=""),
            json.dumps(secret, ensure_ascii=False)[1:-1],
            repr(secret)[1:-1],
        }
        for variant in variants:
            if variant:
                text = text.replace(variant, "[REDACTED]")
    text = _SENSITIVE_QUERY.sub(r"\1[REDACTED]", text)
    text = _SENSITIVE_HEADER.sub(r"\1\2[REDACTED]", text)
    return _KNOWN_KEY_SHAPE.sub("[REDACTED]", text)


def _invalid_url(error_type: type[Exception], label: str, reason: str):
    raise error_type(f"{label}不安全：{reason}")


def _parse_service_url(
    value: str,
    *,
    label: str,
    error_type: type[Exception],
    allow_query: bool = False,
):
    url = str(value or "").strip()
    if not url:
        _invalid_url(error_type, label, "地址为空")
    if "\\" in url or any(char.isspace() or ord(char) < 32 for char in url):
        _invalid_url(error_type, label, "地址包含空白符或反斜杠")
    try:
        parsed = urlsplit(url)
        hostname = parsed.hostname
        parsed.port  # Validate malformed or out-of-range ports.
    except ValueError as exc:
        raise error_type(f"{label}不安全：端口或主机名无效") from exc
    if not parsed.scheme or not parsed.netloc or not hostname:
        _invalid_url(error_type, label, "必须是完整的网络地址")
    if parsed.username is not None or parsed.password is not None or "@" in parsed.netloc:
        _invalid_url(error_type, label, "不能包含用户名或密码")
    if parsed.fragment:
        _invalid_url(error_type, label, "不能包含 # 片段")
    if parsed.query and not allow_query:
        _invalid_url(error_type, label, "不能包含查询参数")
    return url, parsed


def validate_api_base_url(
    value: str,
    *,
    label: str,
    error_type: type[Exception],
    key_present: bool = True,
    allow_keyless_loopback_http: bool = False,
) -> str:
    """Validate a user-configurable API base URL and return it unchanged."""
    url, parsed = _parse_service_url(value, label=label, error_type=error_type)
    scheme = parsed.scheme.lower()
    if scheme == "https":
        return url.rstrip("/")
    if scheme == "http" and allow_keyless_loopback_http and not key_present:
        host = parsed.hostname or ""
        loopback = host.lower() == "localhost"
        if not loopback:
            try:
                loopback = ipaddress.ip_address(host).is_loopback
            except ValueError:
                loopback = False
        if loopback:
            return url.rstrip("/")
    if allow_keyless_loopback_http:
        _invalid_url(
            error_type,
            label,
            "必须使用 HTTPS；仅无密钥的 localhost/127.0.0.1/[::1] 可使用 HTTP",
        )
    _invalid_url(error_type, label, "必须使用 HTTPS")


def validate_same_origin_https(
    value: str,
    base_url: str,
    *,
    label: str,
    error_type: type[Exception],
) -> str:
    """Accept an HTTPS result URL only when it has the exact base origin."""
    url, parsed = _parse_service_url(
        value,
        label=label,
        error_type=error_type,
        allow_query=True,
    )
    _base, base = _parse_service_url(
        base_url,
        label=label,
        error_type=error_type,
    )
    if parsed.scheme.lower() != "https":
        _invalid_url(error_type, label, "必须使用 HTTPS")

    def origin(parts):
        port = parts.port or (443 if parts.scheme.lower() == "https" else 80)
        return parts.scheme.lower(), (parts.hostname or "").lower(), port

    if origin(parsed) != origin(base):
        _invalid_url(error_type, label, "返回地址与配置的 Endpoint 不同源")
    return url


def _content_length(response) -> int | None:
    headers = getattr(response, "headers", None)
    if not isinstance(headers, Mapping):
        return None
    raw = headers.get("Content-Length")
    try:
        value = int(raw)
    except (TypeError, ValueError):
        return None
    return value if value >= 0 else None


def _close_response(response) -> None:
    close = getattr(response, "close", None)
    if callable(close):
        try:
            close()
        except (AttributeError, OSError):
            # A synthetic Response used by tests may not have a raw socket.
            pass


def read_limited_bytes(
    response,
    *,
    limit: int,
    label: str,
    error_type: type[Exception],
) -> bytes:
    """Read at most ``limit`` decompressed bytes from a streamed response."""
    declared = _content_length(response)
    if declared is not None and declared > limit:
        _close_response(response)
        raise error_type(f"{label}响应过大（上限 {limit // 1024} KiB）")

    # Unit-test doubles expose ``json()``/``text`` but do not have a real raw stream.
    if not isinstance(response, requests.Response):
        content = getattr(response, "content", None)
        if isinstance(content, (bytes, bytearray)):
            body = bytes(content)
        else:
            value = getattr(response, "text", "")
            body = value.encode("utf-8") if isinstance(value, str) else b""
        if len(body) > limit:
            raise error_type(f"{label}响应过大（上限 {limit // 1024} KiB）")
        return body

    preloaded = getattr(response, "_content", None)
    if isinstance(preloaded, bytes) and getattr(response, "raw", None) is None:
        _close_response(response)
        if len(preloaded) > limit:
            raise error_type(f"{label}响应过大（上限 {limit // 1024} KiB）")
        return preloaded

    body = bytearray()
    try:
        for chunk in response.iter_content(chunk_size=64 * 1024):
            if not chunk:
                continue
            body.extend(chunk)
            if len(body) > limit:
                raise error_type(f"{label}响应过大（上限 {limit // 1024} KiB）")
        return bytes(body)
    finally:
        _close_response(response)


def json_response(
    response,
    *,
    label: str,
    error_type: type[Exception],
    limit: int = MAX_JSON_BYTES,
):
    """Parse a bounded JSON response without buffering an unlimited body."""
    if not isinstance(response, requests.Response):
        try:
            payload = response.json()
        except (TypeError, ValueError) as exc:
            raise error_type(f"{label}返回了无法解析的 JSON") from exc
        try:
            size = len(json.dumps(payload, ensure_ascii=False).encode("utf-8"))
        except (TypeError, ValueError):
            size = 0
        if size > limit:
            raise error_type(f"{label}响应过大（上限 {limit // 1024} KiB）")
        return payload

    body = read_limited_bytes(response, limit=limit, label=label, error_type=error_type)
    try:
        return json.loads(body)
    except (UnicodeDecodeError, ValueError) as exc:
        raise error_type(f"{label}返回了无法解析的 JSON") from exc


def text_response(
    response,
    *,
    label: str,
    error_type: type[Exception],
    limit: int = MAX_TEXT_BYTES,
) -> str:
    if not isinstance(response, requests.Response):
        value = getattr(response, "text", "")
        text = value if isinstance(value, str) else ""
        if len(text.encode("utf-8")) > limit:
            raise error_type(f"{label}响应过大（上限 {limit // 1024} KiB）")
        return text
    body = read_limited_bytes(response, limit=limit, label=label, error_type=error_type)
    encoding = response.encoding or "utf-8"
    try:
        return body.decode(encoding)
    except (LookupError, UnicodeDecodeError) as exc:
        raise error_type(f"{label}返回了无法解码的文本") from exc


def ensure_success(
    response,
    *,
    label: str,
    error_type: type[Exception],
    secrets: Sequence[str] = (),
) -> None:
    """Reject redirects and errors, reading only a small diagnostic body."""
    status = getattr(response, "status_code", None)
    if not isinstance(status, int):
        response.raise_for_status()
        return
    if 200 <= status < 300:
        return
    try:
        detail = text_response(
            response,
            label=label,
            error_type=error_type,
            limit=MAX_ERROR_BYTES,
        ).strip().replace("\r", " ").replace("\n", " ")
    except Exception:
        detail = ""
    # 必须先按完整密钥脱敏再截断；反过来会留下恰好跨过截断边界的密钥前缀。
    detail = redact_sensitive(detail, secrets)[:300]
    suffix = f"：{detail}" if detail else ""
    if 300 <= status < 400:
        raise error_type(f"{label}拒绝了不安全的 HTTP 重定向（{status}）{suffix}")
    raise error_type(f"{label}请求失败（HTTP {status}）{suffix}")


__all__ = [
    "MAX_JSON_BYTES",
    "configured_secrets",
    "ensure_success",
    "json_response",
    "redact_sensitive",
    "text_response",
    "validate_api_base_url",
    "validate_same_origin_https",
]
