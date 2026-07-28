"""Validate the locked base runtime used for tests and release builds."""
from __future__ import annotations

import argparse
import sys
from importlib.metadata import PackageNotFoundError, version


LOCKED_RUNTIME = {
    "certifi": "2025.4.26",
    "charset-normalizer": "3.4.2",
    "idna": "3.15",
    "mss": "10.1.0",
    "Pillow": "12.3.0",
    "PySide6": "6.5.1.1",
    "PySide6-Addons": "6.5.1.1",
    "PySide6-Essentials": "6.5.1.1",
    "requests": "2.33.0",
    "shiboken6": "6.5.1.1",
    "typing_extensions": "4.16.0",
    "urllib3": "2.7.0",
    "winrt-runtime": "3.2.1",
    "winrt-Windows.Foundation": "3.2.1",
    "winrt-Windows.Foundation.Collections": "3.2.1",
    "winrt-Windows.Globalization": "3.2.1",
    "winrt-Windows.Graphics.Imaging": "3.2.1",
    "winrt-Windows.Media.Ocr": "3.2.1",
    "winrt-Windows.Security.Cryptography": "3.2.1",
    "winrt-Windows.Storage.Streams": "3.2.1",
}


def check(strict_lock: bool = False) -> list[str]:
    errors: list[str] = []
    installed: dict[str, str] = {}
    for package in LOCKED_RUNTIME:
        try:
            installed[package] = version(package)
        except PackageNotFoundError:
            errors.append(f"missing package: {package}")

    if strict_lock:
        for package, expected in LOCKED_RUNTIME.items():
            actual = installed.get(package)
            if actual and actual != expected:
                errors.append(f"{package}: expected {expected}, found {actual}")
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--strict-lock",
        action="store_true",
        help="also require the versions recorded in requirements.lock",
    )
    args = parser.parse_args(argv)
    errors = check(args.strict_lock)
    if errors:
        for error in errors:
            print(f"[environment] ERROR: {error}", file=sys.stderr)
        print(
            "[environment] Recreate the environment with requirements-build.lock.",
            file=sys.stderr,
        )
        return 1
    print("[environment] Runtime dependency lock is valid.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
