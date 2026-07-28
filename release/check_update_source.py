"""Validate the update trust root embedded in a packaged release."""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any


MAX_SOURCE_BYTES = 4096
_GITHUB_REPOSITORY = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")


class UpdateSourceError(ValueError):
    """Raised when a packaged update source is missing or inconsistent."""


def expected_update_source(repository: str, channel: str) -> dict[str, str]:
    repository = str(repository or "").strip()
    if _GITHUB_REPOSITORY.fullmatch(repository) is None:
        raise UpdateSourceError("repository must use OWNER/REPO format")
    owner, name = repository.split("/", 1)
    if owner in (".", "..") or name in (".", ".."):
        raise UpdateSourceError("repository contains an invalid name")
    if channel not in ("stable", "preview"):
        raise UpdateSourceError("channel must be stable or preview")

    repository_url = f"https://github.com/{owner}/{name}"
    return {
        "manifest_url": (
            f"{repository_url}/releases/latest/download/update-manifest.json"
        ),
        "repository_url": repository_url,
        "channel": channel,
    }


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise UpdateSourceError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def validate_update_source(
    source_path: Path,
    repository: str,
    channel: str,
) -> dict[str, str]:
    try:
        payload = source_path.read_bytes()
    except OSError as exc:
        raise UpdateSourceError(f"cannot read embedded update source: {source_path}") from exc
    if not payload:
        raise UpdateSourceError("embedded update source is empty")
    if len(payload) > MAX_SOURCE_BYTES:
        raise UpdateSourceError("embedded update source is unexpectedly large")

    try:
        document = json.loads(
            payload.decode("utf-8"),
            object_pairs_hook=_reject_duplicate_keys,
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise UpdateSourceError("embedded update source is not valid UTF-8 JSON") from exc

    expected = expected_update_source(repository, channel)
    if document != expected:
        raise UpdateSourceError(
            "embedded update source does not match the release repository, "
            "channel, and latest manifest URL"
        )
    return expected


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate the update source embedded by build.py.",
    )
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--channel", required=True)
    args = parser.parse_args(argv)

    try:
        expected = validate_update_source(
            args.source,
            args.repository,
            args.channel,
        )
    except UpdateSourceError as exc:
        print(f"[release] 更新源校验失败：{exc}", file=sys.stderr)
        return 1

    print(
        "[release] 更新源校验通过："
        f"{expected['repository_url']} ({expected['channel']})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
