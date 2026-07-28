"""Read-only GitHub Release update checks.

The application deliberately never downloads or launches release artifacts.  An
update result includes validated artifact metadata, but the only URL the UI may
open is a repository-bound GitHub Release page.
"""
from __future__ import annotations

import json
import re
import time
from dataclasses import dataclass
from datetime import datetime
from threading import Event
from typing import Callable
from urllib.parse import urlparse

import requests
from urllib3.util import Timeout

from . import __version__


MANIFEST_LIMIT = 256 * 1024
CHECK_TOTAL_TIMEOUT = 30.0
CHECK_TIMEOUT = (2, 10)
UPDATE_CHANNELS = ("stable", "preview")
_SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
_GITHUB_NAME = re.compile(r"^[A-Za-z0-9_.-]+$")
_ARTIFACT_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,254}$")
_SHA256 = re.compile(r"^[0-9A-Fa-f]{64}$")
_MANIFEST_NAME = "update-manifest.json"


class UpdateError(RuntimeError):
    pass


class UpdateCancelled(UpdateError):
    pass


@dataclass(frozen=True)
class GitHubRepository:
    owner: str
    name: str

    @property
    def url(self) -> str:
        return f"https://github.com/{self.owner}/{self.name}"

    def same_as(self, other: "GitHubRepository") -> bool:
        return (
            self.owner.casefold() == other.owner.casefold()
            and self.name.casefold() == other.name.casefold()
        )


@dataclass(frozen=True)
class ArtifactInfo:
    name: str
    url: str
    size: int
    sha256: str


@dataclass(frozen=True)
class UpdateInfo:
    version: str
    channel: str
    release_url: str
    published_at: str
    artifact: ArtifactInfo


def _https_url(url: object, label: str) -> str:
    if not isinstance(url, str):
        raise UpdateError(f"{label}不是有效地址")
    parsed = urlparse(url)
    try:
        port = parsed.port
    except ValueError as exc:
        raise UpdateError(f"{label}端口无效") from exc
    if (
        parsed.scheme.lower() != "https"
        or not parsed.hostname
        or port not in (None, 443)
        or parsed.username is not None
        or parsed.password is not None
        or parsed.fragment
    ):
        raise UpdateError(f"{label}必须是无账号信息和片段的 HTTPS 地址")
    return url


def _standard_https_port(parsed, label: str) -> bool:
    try:
        return parsed.port in (None, 443)
    except ValueError as exc:
        raise UpdateError(f"{label}端口无效") from exc


def github_repository_url(url: object) -> GitHubRepository:
    """Parse an exact GitHub repository homepage URL."""
    value = _https_url(url, "GitHub 仓库地址")
    parsed = urlparse(value)
    parts = [part for part in parsed.path.split("/") if part]
    if (
        parsed.hostname.casefold() != "github.com"
        or not _standard_https_port(parsed, "GitHub 仓库地址")
        or parsed.query
        or len(parts) != 2
        or any(_GITHUB_NAME.fullmatch(part) is None for part in parts)
        or parts[0] in (".", "..")
        or parts[1] in (".", "..")
    ):
        raise UpdateError("GitHub 仓库地址格式无效")
    return GitHubRepository(parts[0], parts[1])


def github_manifest_repository(url: object) -> GitHubRepository:
    """Return the repository pinned by a supported GitHub Release asset URL."""
    value = _https_url(url, "更新清单地址")
    parsed = urlparse(value)
    parts = [part for part in parsed.path.split("/") if part]
    valid_latest = (
        len(parts) == 6
        and parts[2:5] == ["releases", "latest", "download"]
        and parts[5] == _MANIFEST_NAME
    )
    valid_tag = (
        len(parts) == 6
        and parts[2] == "releases"
        and parts[3] == "download"
        and bool(parts[4])
        and parts[5] == _MANIFEST_NAME
    )
    if (
        parsed.hostname.casefold() != "github.com"
        or not _standard_https_port(parsed, "更新清单地址")
        or parsed.query
        or not (valid_latest or valid_tag)
        or any(_GITHUB_NAME.fullmatch(part) is None for part in parts[:2])
    ):
        raise UpdateError("更新清单必须是 GitHub Release 中的 update-manifest.json")
    return GitHubRepository(parts[0], parts[1])


def _version_parts(value: str) -> tuple[tuple[object, ...], bool]:
    if len(value) > 128:
        raise UpdateError("版本号过长")
    match = _SEMVER.fullmatch(value)
    if match is None:
        raise UpdateError(f"版本号不符合 SemVer：{value}")
    major, minor, patch, prerelease = match.groups()
    if prerelease is None:
        pre_key: tuple[object, ...] = (1,)
    else:
        identifiers: list[tuple[int, object]] = []
        for part in prerelease.split("."):
            if part.isdigit():
                if len(part) > 1 and part.startswith("0"):
                    raise UpdateError(f"版本号不符合 SemVer：{value}")
                identifiers.append((0, int(part)))
            else:
                identifiers.append((1, part))
        pre_key = (0, *identifiers)
    return (int(major), int(minor), int(patch), pre_key), prerelease is not None


def _version_key(value: str) -> tuple[object, ...]:
    return _version_parts(value)[0]


def is_newer_version(candidate: str, current: str = __version__) -> bool:
    return _version_key(candidate) > _version_key(current)


def _validate_channel(channel: object) -> str:
    if channel not in UPDATE_CHANNELS:
        raise UpdateError("更新通道必须是 stable 或 preview")
    return str(channel)


def release_url_for(repository: GitHubRepository, version: str) -> str:
    _version_key(version)
    return f"{repository.url}/releases/tag/v{version}"


def _parse_artifact(
    value: object,
    repository: GitHubRepository,
    version: str,
) -> ArtifactInfo:
    if not isinstance(value, dict):
        raise UpdateError("更新清单缺少安装包信息")

    name = value.get("name")
    if not isinstance(name, str) or _ARTIFACT_NAME.fullmatch(name) is None:
        raise UpdateError("更新清单中的安装包文件名无效")

    artifact_url = _https_url(value.get("url"), "安装包地址")
    parsed = urlparse(artifact_url)
    parts = parsed.path.split("/")
    expected_repository = GitHubRepository(parts[1], parts[2]) if len(parts) == 7 else None
    if (
        parsed.hostname.casefold() != "github.com"
        or not _standard_https_port(parsed, "安装包地址")
        or parsed.query
        or len(parts) != 7
        or parts[0] != ""
        or parts[3:5] != ["releases", "download"]
        or expected_repository is None
        or not expected_repository.same_as(repository)
        or parts[5] != f"v{version}"
        or parts[6] != name
    ):
        raise UpdateError("安装包地址必须属于配置的 GitHub 仓库、版本和文件名")

    size = value.get("size")
    if type(size) is not int or not 0 <= size <= (1 << 63) - 1:
        raise UpdateError("更新清单中的安装包大小无效")

    sha256 = value.get("sha256")
    if not isinstance(sha256, str) or _SHA256.fullmatch(sha256) is None:
        raise UpdateError("更新清单中的安装包 SHA-256 无效")

    return ArtifactInfo(name, artifact_url, size, sha256.lower())


def parse_manifest(payload: object, repository_url: str) -> UpdateInfo:
    if not isinstance(payload, dict):
        raise UpdateError("更新清单根节点必须是对象")
    if payload.get("schema_version") != 2 or payload.get("product") != "ScreenTranslate":
        raise UpdateError("更新清单版本或产品名不受支持")

    repository = github_repository_url(repository_url)
    platform = payload.get("platform")
    if not isinstance(platform, dict) or platform.get("os") != "windows":
        raise UpdateError("更新清单不是 Windows 版本")
    if platform.get("arch") not in ("x86_64", "amd64"):
        raise UpdateError("更新清单不是 x64 版本")

    version = payload.get("version")
    if not isinstance(version, str):
        raise UpdateError("更新清单缺少版本号")
    _key, is_prerelease = _version_parts(version)
    channel = _validate_channel(payload.get("channel"))
    if (channel == "stable") == is_prerelease:
        raise UpdateError("更新清单的版本号与发布通道不一致")

    release_url = payload.get("release_url")
    expected_release_url = release_url_for(repository, version)
    if release_url != expected_release_url:
        raise UpdateError("更新页面不属于配置的 GitHub 仓库或版本")

    published_at = payload.get("published_at", "")
    if not isinstance(published_at, str) or len(published_at) > 64:
        raise UpdateError("更新清单中的发布时间无效")
    if published_at:
        try:
            published = datetime.fromisoformat(published_at.replace("Z", "+00:00"))
        except ValueError as exc:
            raise UpdateError("更新清单中的发布时间无效") from exc
        if published.tzinfo is None or published.utcoffset() is None:
            raise UpdateError("更新清单中的发布时间必须包含时区")
    artifact = _parse_artifact(payload.get("artifact"), repository, version)
    return UpdateInfo(version, channel, expected_release_url, published_at, artifact)


def _deadline_after(total_timeout: float, label: str) -> float:
    if total_timeout <= 0:
        raise UpdateError(f"{label}总时限必须大于 0")
    return time.monotonic() + total_timeout


def _ensure_active(cancel_event: Event | None, deadline: float, label: str) -> None:
    if cancel_event is not None and cancel_event.is_set():
        raise UpdateCancelled(f"{label}已取消")
    if time.monotonic() >= deadline:
        raise UpdateError(f"{label}超过总时限")


def _request_timeout(limits: tuple[float, float], deadline: float) -> Timeout:
    remaining = max(0.001, deadline - time.monotonic())
    return Timeout(
        total=remaining,
        connect=min(limits[0], remaining),
        read=min(limits[1], remaining),
    )


def _content_length(response: object) -> int | None:
    headers = getattr(response, "headers", None)
    if headers is None or not callable(getattr(headers, "get", None)):
        return None
    raw = headers.get("Content-Length")
    if raw in (None, ""):
        return None
    try:
        value = int(raw)
    except (TypeError, ValueError) as exc:
        raise UpdateError("服务器返回的 Content-Length 无效") from exc
    if value < 0:
        raise UpdateError("服务器返回的 Content-Length 无效")
    return value


def _response_bytes(
    response: object,
    *,
    cancel_event: Event | None,
    deadline: float,
) -> bytes:
    declared = _content_length(response)
    if declared is not None and declared > MANIFEST_LIMIT:
        raise UpdateError("更新清单过大")
    iterator = getattr(response, "iter_content", None)
    if not callable(iterator):
        raise UpdateError("更新清单响应不支持流式读取")

    content = bytearray()
    for chunk in iterator(chunk_size=64 * 1024):
        _ensure_active(cancel_event, deadline, "检查更新")
        if not chunk:
            continue
        if len(content) + len(chunk) > MANIFEST_LIMIT:
            raise UpdateError("更新清单过大")
        content.extend(chunk)
    _ensure_active(cancel_event, deadline, "检查更新")
    return bytes(content)


def _validate_response_location(response: object, original_url: str) -> None:
    """Allow only HTTPS redirects served by GitHub's asset infrastructure."""
    allowed_hosts = {"github.com", "objects.githubusercontent.com"}
    chain = [*getattr(response, "history", ()), response]
    for item in chain:
        final_url = getattr(item, "url", original_url)
        _https_url(final_url, "更新清单响应地址")
        parsed = urlparse(final_url)
        host = (parsed.hostname or "").casefold()
        if (
            not _standard_https_port(parsed, "更新清单响应地址")
            or (host not in allowed_hosts and not host.endswith(".githubusercontent.com"))
        ):
            raise UpdateError("更新清单被重定向到非 GitHub 地址")


def check_for_update(
    manifest_url: str,
    *,
    repository_url: str,
    channel: str = "stable",
    current_version: str = __version__,
    session: object | None = None,
    cancel_event: Event | None = None,
    total_timeout: float = CHECK_TOTAL_TIMEOUT,
    response_observer: Callable[[object | None], None] | None = None,
) -> UpdateInfo | None:
    source_repository = github_manifest_repository(manifest_url)
    expected_repository = github_repository_url(repository_url)
    if not source_repository.same_as(expected_repository):
        raise UpdateError("更新清单地址不属于内置的 GitHub 仓库")
    requested_channel = _validate_channel(channel)
    _version_key(current_version)

    deadline = _deadline_after(total_timeout, "检查更新")
    client = session or requests
    response = None
    try:
        _ensure_active(cancel_event, deadline, "检查更新")
        response = client.get(
            manifest_url,
            timeout=_request_timeout(CHECK_TIMEOUT, deadline),
            headers={
                "Accept": "application/json",
                "User-Agent": f"ScreenTranslate/{current_version}",
            },
            stream=True,
        )
        if response_observer is not None:
            response_observer(response)
        _ensure_active(cancel_event, deadline, "检查更新")
        response.raise_for_status()
        _validate_response_location(response, manifest_url)
        try:
            payload = json.loads(
                _response_bytes(
                    response,
                    cancel_event=cancel_event,
                    deadline=deadline,
                ).decode("utf-8-sig")
            )
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise UpdateError("更新清单不是有效的 UTF-8 JSON") from exc
        info = parse_manifest(payload, expected_repository.url)
        if requested_channel == "stable" and info.channel != "stable":
            return None
        return info if is_newer_version(info.version, current_version) else None
    except requests.RequestException as exc:
        if cancel_event is not None and cancel_event.is_set():
            raise UpdateCancelled("检查更新已取消") from exc
        raise UpdateError(f"检查更新失败（{type(exc).__name__}）") from exc
    finally:
        if response is not None and callable(getattr(response, "close", None)):
            response.close()
        if response_observer is not None:
            response_observer(None)
