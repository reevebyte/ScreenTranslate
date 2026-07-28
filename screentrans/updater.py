"""Repository-pinned GitHub Release checks and verified installer downloads."""
from __future__ import annotations

import ctypes
import hashlib
import json
import os
import re
import shutil
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from threading import Event
from typing import Callable
from urllib.parse import urlparse

import requests
from urllib3.util import Timeout

from . import __version__


MANIFEST_LIMIT = 256 * 1024
ARTIFACT_LIMIT = 512 * 1024 * 1024
CHECK_TOTAL_TIMEOUT = 30.0
CHECK_TIMEOUT = (2, 10)
DOWNLOAD_TOTAL_TIMEOUT = 10 * 60.0
DOWNLOAD_TIMEOUT = (5, 30)
DOWNLOAD_CHUNK_SIZE = 256 * 1024
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
_INSTALL_HELPER_FLAG = "--apply-update"
_INSTALL_HELPER_OPTIONS = {
    "--update-path",
    "--update-version",
    "--update-size",
    "--update-sha256",
    "--update-repository",
    "--parent-pid",
}


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
    expected_name = f"ScreenTranslate-{version}-setup-x64.exe"
    if name != expected_name:
        raise UpdateError("更新清单中的安装包文件名与版本不一致")

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
    if type(size) is not int or not 1 <= size <= ARTIFACT_LIMIT:
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


def _validate_response_location(
    response: object,
    original_url: str,
    label: str = "更新清单",
) -> None:
    """Allow only HTTPS redirects served by GitHub's asset infrastructure."""
    allowed_hosts = {"github.com", "objects.githubusercontent.com"}
    chain = [*getattr(response, "history", ()), response]
    for item in chain:
        final_url = getattr(item, "url", original_url)
        _https_url(final_url, f"{label}响应地址")
        parsed = urlparse(final_url)
        host = (parsed.hostname or "").casefold()
        if (
            not _standard_https_port(parsed, f"{label}响应地址")
            or (host not in allowed_hosts and not host.endswith(".githubusercontent.com"))
        ):
            raise UpdateError(f"{label}被重定向到非 GitHub 地址")


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


def default_update_dir() -> Path:
    base = os.environ.get("LOCALAPPDATA") or tempfile.gettempdir()
    return Path(base) / "ScreenTranslate" / "updates"


def _is_reparse_point(path: Path) -> bool:
    try:
        stat_result = path.lstat()
    except FileNotFoundError:
        return False
    except OSError as exc:
        raise UpdateError("无法检查更新缓存路径") from exc
    return path.is_symlink() or bool(
        getattr(stat_result, "st_file_attributes", 0) & 0x400
    )


def _remove_cache_entry(path: Path) -> None:
    try:
        if path.is_symlink():
            path.unlink()
        elif _is_reparse_point(path):
            if path.is_dir():
                path.rmdir()
            else:
                path.unlink()
        elif path.is_dir():
            shutil.rmtree(path)
        else:
            path.unlink()
    except (FileNotFoundError, OSError, UpdateError):
        # A running installer can temporarily lock its own cached executable.
        # Cleanup is best-effort and will be retried before the next download.
        pass


def _prune_update_cache(root: Path, keep_directory: Path) -> None:
    try:
        entries = list(root.iterdir())
    except OSError:
        return
    for entry in entries:
        if entry != keep_directory:
            _remove_cache_entry(entry)
            continue
        try:
            current_entries = list(entry.iterdir())
        except OSError:
            continue
        for current_entry in current_entries:
            if current_entry.name.endswith(".part"):
                _remove_cache_entry(current_entry)


def _validated_update_info(
    info: UpdateInfo,
    repository_url: str,
    *,
    current_version: str = __version__,
) -> ArtifactInfo:
    if not isinstance(info, UpdateInfo):
        raise UpdateError("待下载的更新信息无效")
    repository = github_repository_url(repository_url)
    _version_key(current_version)
    _key, is_prerelease = _version_parts(info.version)
    channel = _validate_channel(info.channel)
    if (channel == "stable") == is_prerelease:
        raise UpdateError("待下载版本与发布通道不一致")
    if info.release_url != release_url_for(repository, info.version):
        raise UpdateError("待下载更新不属于内置的 GitHub 仓库")
    artifact = _parse_artifact(
        {
            "name": info.artifact.name,
            "url": info.artifact.url,
            "size": info.artifact.size,
            "sha256": info.artifact.sha256,
        },
        repository,
        info.version,
    )
    if artifact != info.artifact:
        raise UpdateError("待下载安装包信息不一致")
    if not is_newer_version(info.version, current_version):
        raise UpdateError("待安装版本不高于当前版本")
    return artifact


def _update_target(
    info: UpdateInfo,
    download_dir: Path | None,
    *,
    create: bool,
) -> Path:
    root_path = Path(download_dir) if download_dir is not None else default_update_dir()
    root_path = root_path.expanduser()
    if _is_reparse_point(root_path):
        raise UpdateError("更新缓存目录不能是链接或 Windows 重解析点")
    root = root_path.resolve()
    if create:
        try:
            root.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            raise UpdateError("无法创建更新缓存目录") from exc
        if _is_reparse_point(root):
            raise UpdateError("更新缓存目录不能是链接或 Windows 重解析点")
    version_dir = root / f"v{info.version}"
    if _is_reparse_point(version_dir):
        raise UpdateError("版本缓存目录不能是链接或 Windows 重解析点")
    if create:
        try:
            version_dir.mkdir(parents=False, exist_ok=True)
        except OSError as exc:
            raise UpdateError("无法创建版本缓存目录") from exc
        if _is_reparse_point(version_dir):
            raise UpdateError("版本缓存目录不能是链接或 Windows 重解析点")
    if version_dir.exists() and not version_dir.is_dir():
        raise UpdateError("版本缓存路径不是目录")
    if version_dir.exists() and version_dir.resolve() != version_dir:
        raise UpdateError("版本缓存目录超出更新缓存路径")
    target = version_dir / info.artifact.name
    if target.parent != version_dir or version_dir.parent != root:
        raise UpdateError("安装包保存路径无效")
    return target


def _digest_file(
    path: Path,
    *,
    cancel_event: Event | None = None,
    progress_callback: Callable[[int, int], None] | None = None,
    expected_size: int | None = None,
) -> tuple[int, str]:
    digest = hashlib.sha256()
    received = 0
    try:
        with path.open("rb") as handle:
            while True:
                if cancel_event is not None and cancel_event.is_set():
                    raise UpdateCancelled("下载更新已取消")
                chunk = handle.read(DOWNLOAD_CHUNK_SIZE)
                if not chunk:
                    break
                received += len(chunk)
                if expected_size is not None and received > expected_size:
                    raise UpdateError("安装包大小与更新清单不一致")
                digest.update(chunk)
                if progress_callback is not None and expected_size is not None:
                    progress_callback(received, expected_size)
    except OSError as exc:
        raise UpdateError(f"无法读取安装包（{type(exc).__name__}）") from exc
    return received, digest.hexdigest()


def _verify_installer(
    path: Path,
    artifact: ArtifactInfo,
    *,
    cancel_event: Event | None = None,
    progress_callback: Callable[[int, int], None] | None = None,
) -> None:
    size, digest = _digest_file(
        path,
        cancel_event=cancel_event,
        progress_callback=progress_callback,
        expected_size=artifact.size,
    )
    if size != artifact.size:
        raise UpdateError("安装包大小与更新清单不一致")
    if digest.casefold() != artifact.sha256.casefold():
        raise UpdateError("安装包 SHA-256 校验失败")


def _mark_as_internet_download(path: Path, source_url: str) -> None:
    if os.name != "nt":
        return
    _https_url(source_url, "安装包地址")
    zone_data = (
        "[ZoneTransfer]\r\n"
        "ZoneId=3\r\n"
        f"HostUrl={source_url}\r\n"
    )
    try:
        with open(f"{path}:Zone.Identifier", "w", encoding="utf-8", newline="") as stream:
            stream.write(zone_data)
    except OSError as exc:
        raise UpdateError("无法标记安装包的网络来源，已停止自动安装") from exc


def download_update(
    info: UpdateInfo,
    *,
    repository_url: str,
    current_version: str = __version__,
    download_dir: Path | None = None,
    session: object | None = None,
    cancel_event: Event | None = None,
    total_timeout: float = DOWNLOAD_TOTAL_TIMEOUT,
    progress_callback: Callable[[int, int], None] | None = None,
    response_observer: Callable[[object | None], None] | None = None,
) -> Path:
    artifact = _validated_update_info(
        info,
        repository_url,
        current_version=current_version,
    )
    target = _update_target(info, download_dir, create=True)
    _prune_update_cache(target.parent.parent, target.parent)
    if target.exists():
        if not target.is_file() or _is_reparse_point(target):
            raise UpdateError("安装包缓存路径不是普通文件")
        try:
            _verify_installer(
                target,
                artifact,
                cancel_event=cancel_event,
                progress_callback=progress_callback,
            )
            _mark_as_internet_download(target, artifact.url)
            return target
        except UpdateCancelled:
            raise
        except UpdateError:
            try:
                target.unlink()
            except OSError as exc:
                raise UpdateError("无法清理损坏的安装包缓存") from exc

    deadline = _deadline_after(total_timeout, "下载更新")
    client = session or requests
    response = None
    part_path: Path | None = None
    try:
        _ensure_active(cancel_event, deadline, "下载更新")
        response = client.get(
            artifact.url,
            timeout=_request_timeout(DOWNLOAD_TIMEOUT, deadline),
            headers={
                "Accept": "application/octet-stream",
                "User-Agent": f"ScreenTranslate/{current_version}",
            },
            stream=True,
        )
        if response_observer is not None:
            response_observer(response)
        _ensure_active(cancel_event, deadline, "下载更新")
        response.raise_for_status()
        _validate_response_location(response, artifact.url, "安装包")
        declared = _content_length(response)
        if declared is not None and declared != artifact.size:
            raise UpdateError("服务器返回的安装包大小与更新清单不一致")
        iterator = getattr(response, "iter_content", None)
        if not callable(iterator):
            raise UpdateError("安装包响应不支持流式读取")

        digest = hashlib.sha256()
        received = 0
        with tempfile.NamedTemporaryFile(
            mode="wb",
            prefix=f"{artifact.name}.",
            suffix=".part",
            dir=target.parent,
            delete=False,
        ) as output:
            part_path = Path(output.name)
            for chunk in iterator(chunk_size=DOWNLOAD_CHUNK_SIZE):
                _ensure_active(cancel_event, deadline, "下载更新")
                if not chunk:
                    continue
                received += len(chunk)
                if received > artifact.size:
                    raise UpdateError("安装包大小与更新清单不一致")
                output.write(chunk)
                digest.update(chunk)
                if progress_callback is not None:
                    progress_callback(received, artifact.size)
            output.flush()
            os.fsync(output.fileno())

        _ensure_active(cancel_event, deadline, "下载更新")
        if received != artifact.size:
            raise UpdateError("安装包大小与更新清单不一致")
        if digest.hexdigest().casefold() != artifact.sha256.casefold():
            raise UpdateError("安装包 SHA-256 校验失败")
        _mark_as_internet_download(part_path, artifact.url)
        os.replace(part_path, target)
        part_path = None
        return target
    except UpdateCancelled:
        raise
    except requests.RequestException as exc:
        if cancel_event is not None and cancel_event.is_set():
            raise UpdateCancelled("下载更新已取消") from exc
        raise UpdateError(f"下载更新失败（{type(exc).__name__}）") from exc
    except OSError as exc:
        raise UpdateError(f"保存安装包失败（{type(exc).__name__}）") from exc
    finally:
        if response is not None and callable(getattr(response, "close", None)):
            response.close()
        if response_observer is not None:
            response_observer(None)
        if part_path is not None:
            try:
                part_path.unlink()
            except OSError:
                pass


def verify_downloaded_installer(
    path: Path | str,
    info: UpdateInfo,
    *,
    repository_url: str,
    current_version: str = __version__,
    download_dir: Path | None = None,
) -> Path:
    artifact = _validated_update_info(
        info,
        repository_url,
        current_version=current_version,
    )
    expected = _update_target(info, download_dir, create=False)
    candidate = Path(path)
    if _is_reparse_point(candidate):
        raise UpdateError("安装包不能是链接或 Windows 重解析点")
    try:
        resolved = candidate.resolve(strict=True)
    except OSError as exc:
        raise UpdateError("已下载的安装包不存在") from exc
    if resolved != expected.resolve(strict=False) or not resolved.is_file():
        raise UpdateError("安装包不在受信任的更新目录中")
    _verify_installer(resolved, artifact)
    return resolved


def install_helper_arguments(
    path: Path | str,
    info: UpdateInfo,
    *,
    repository_url: str,
    parent_pid: int,
    current_version: str = __version__,
    download_dir: Path | None = None,
) -> list[str]:
    if type(parent_pid) is not int or parent_pid <= 0:
        raise UpdateError("更新进程编号无效")
    verified = verify_downloaded_installer(
        path,
        info,
        repository_url=repository_url,
        current_version=current_version,
        download_dir=download_dir,
    )
    return [
        _INSTALL_HELPER_FLAG,
        "--update-path", str(verified),
        "--update-version", info.version,
        "--update-size", str(info.artifact.size),
        "--update-sha256", info.artifact.sha256,
        "--update-repository", repository_url,
        "--parent-pid", str(parent_pid),
    ]


def is_install_helper_request(argv: list[str]) -> bool:
    return _INSTALL_HELPER_FLAG in argv


def _parse_install_helper_options(argv: list[str]) -> dict[str, str]:
    if argv.count(_INSTALL_HELPER_FLAG) != 1:
        raise UpdateError("更新辅助进程参数无效")
    index = argv.index(_INSTALL_HELPER_FLAG) + 1
    values: dict[str, str] = {}
    while index < len(argv):
        option = argv[index]
        if option not in _INSTALL_HELPER_OPTIONS or option in values or index + 1 >= len(argv):
            raise UpdateError("更新辅助进程参数无效")
        values[option] = argv[index + 1]
        index += 2
    if set(values) != _INSTALL_HELPER_OPTIONS:
        raise UpdateError("更新辅助进程参数不完整")
    return values


def _wait_for_process_exit(pid: int, timeout_seconds: float = 30.0) -> None:
    if os.name != "nt":
        raise UpdateError("自动安装仅支持 Windows")
    from ctypes import wintypes

    SYNCHRONIZE = 0x00100000
    WAIT_OBJECT_0 = 0
    WAIT_TIMEOUT = 258
    ERROR_INVALID_PARAMETER = 87
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel32.WaitForSingleObject.restype = wintypes.DWORD
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CloseHandle.restype = wintypes.BOOL

    handle = kernel32.OpenProcess(SYNCHRONIZE, False, pid)
    if not handle:
        error = ctypes.get_last_error()
        if error == ERROR_INVALID_PARAMETER:
            return
        raise UpdateError(f"无法等待旧版本退出（Windows 错误 {error}）")
    try:
        result = kernel32.WaitForSingleObject(
            handle,
            max(1, int(timeout_seconds * 1000)),
        )
    finally:
        kernel32.CloseHandle(handle)
    if result == WAIT_TIMEOUT:
        raise UpdateError("旧版本未能及时退出，安装已取消")
    if result != WAIT_OBJECT_0:
        raise UpdateError("等待旧版本退出失败")


def run_install_helper(
    argv: list[str],
    *,
    current_version: str = __version__,
    download_dir: Path | None = None,
    wait_for_exit: Callable[[int], None] | None = None,
    launcher: Callable[[str], object] | None = None,
) -> Path:
    values = _parse_install_helper_options(argv)
    try:
        size = int(values["--update-size"])
        parent_pid = int(values["--parent-pid"])
    except ValueError as exc:
        raise UpdateError("更新辅助进程数字参数无效") from exc
    version = values["--update-version"]
    repository = github_repository_url(values["--update-repository"])
    _key, is_prerelease = _version_parts(version)
    channel = "preview" if is_prerelease else "stable"
    name = f"ScreenTranslate-{version}-setup-x64.exe"
    info = UpdateInfo(
        version=version,
        channel=channel,
        release_url=release_url_for(repository, version),
        published_at="",
        artifact=ArtifactInfo(
            name=name,
            url=f"{repository.url}/releases/download/v{version}/{name}",
            size=size,
            sha256=values["--update-sha256"],
        ),
    )
    if parent_pid <= 0:
        raise UpdateError("更新进程编号无效")
    waiter = wait_for_exit or _wait_for_process_exit
    waiter(parent_pid)
    verified = verify_downloaded_installer(
        values["--update-path"],
        info,
        repository_url=repository.url,
        current_version=current_version,
        download_dir=download_dir,
    )
    _mark_as_internet_download(verified, info.artifact.url)
    starter = launcher or getattr(os, "startfile", None)
    if not callable(starter):
        raise UpdateError("系统不支持启动安装程序")
    try:
        starter(str(verified))
    except OSError as exc:
        raise UpdateError(f"无法启动安装程序（{type(exc).__name__}）") from exc
    return verified
