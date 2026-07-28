"""Generate the JSON manifest consumed by the in-app updater."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlparse


_CORE_NUMBER = r"(?:0|[1-9][0-9]*)"
_PRERELEASE_PART = r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION_RE = re.compile(
    rf"^{_CORE_NUMBER}\.{_CORE_NUMBER}\.{_CORE_NUMBER}"
    rf"(?:-{_PRERELEASE_PART}(?:\.{_PRERELEASE_PART})*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
GITHUB_NAME_RE = re.compile(r"^[A-Za-z0-9_.-]+$")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def published_at(explicit: str | None) -> str:
    if explicit:
        parsed = datetime.fromisoformat(explicit.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            raise ValueError("--published-at must include a timezone")
        stamp = parsed.astimezone(timezone.utc)
    elif os.environ.get("SOURCE_DATE_EPOCH"):
        stamp = datetime.fromtimestamp(int(os.environ["SOURCE_DATE_EPOCH"]), timezone.utc)
    else:
        stamp = datetime.now(timezone.utc)
    return stamp.isoformat(timespec="seconds").replace("+00:00", "Z")


def build_manifest(
    artifact: Path,
    version: str,
    download_url: str,
    channel: str,
    timestamp: str | None,
) -> dict[str, object]:
    if not artifact.is_file():
        raise FileNotFoundError(f"artifact does not exist: {artifact}")
    if len(version) > 128 or not VERSION_RE.fullmatch(version):
        raise ValueError("version must be SemVer, for example 1.2.3 or 1.2.3-rc.1")
    if channel not in ("stable", "preview"):
        raise ValueError("channel must be stable or preview")
    is_prerelease = "-" in version.split("+", 1)[0]
    if (channel == "stable") == is_prerelease:
        raise ValueError("stable requires a final version and preview requires a prerelease version")

    parsed_url = urlparse(download_url)
    try:
        download_port = parsed_url.port
    except ValueError as exc:
        raise ValueError("download URL contains an invalid port") from exc
    parts = [part for part in parsed_url.path.split("/") if part]
    expected_tag = f"v{version}"
    if (
        parsed_url.scheme != "https"
        or (parsed_url.hostname or "").casefold() != "github.com"
        or download_port not in (None, 443)
        or parsed_url.username is not None
        or parsed_url.password is not None
        or parsed_url.query
        or parsed_url.fragment
        or len(parts) != 6
        or any(GITHUB_NAME_RE.fullmatch(part) is None for part in parts[:2])
        or parts[0] in (".", "..")
        or parts[1] in (".", "..")
        or parts[2:4] != ["releases", "download"]
        or parts[4] != expected_tag
        or parts[5] != artifact.name
    ):
        raise ValueError(
            "download URL must be the matching artifact under "
            "https://github.com/OWNER/REPO/releases/download/vVERSION/"
        )
    repository_url = f"https://github.com/{parts[0]}/{parts[1]}"

    return {
        "schema_version": 2,
        "product": "ScreenTranslate",
        "version": version,
        "channel": channel,
        "published_at": published_at(timestamp),
        "release_url": f"{repository_url}/releases/tag/{expected_tag}",
        "platform": {"os": "windows", "arch": "x86_64", "minimum": "10"},
        "artifact": {
            "name": artifact.name,
            "url": download_url,
            "size": artifact.stat().st_size,
            "sha256": sha256_file(artifact),
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--download-url", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--channel", choices=("stable", "preview"), default="stable")
    parser.add_argument("--published-at")
    args = parser.parse_args(argv)

    manifest = build_manifest(
        args.artifact,
        args.version,
        args.download_url,
        args.channel,
        args.published_at,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"[manifest] wrote {args.output} ({manifest['artifact']['sha256']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
