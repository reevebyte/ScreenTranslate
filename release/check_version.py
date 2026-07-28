"""Ensure the release tag, application version, and installer version cannot diverge."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from screentrans import __version__

from release.generate_manifest import VERSION_RE


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected", required=True, help="SemVer from the release tag")
    args = parser.parse_args(argv)
    if VERSION_RE.fullmatch(args.expected) is None:
        parser.error("--expected must be SemVer")
    if __version__ != args.expected:
        parser.error(
            f"release tag is {args.expected}, but screentrans.__version__ is {__version__}; "
            "update the source version before tagging"
        )
    print(f"[version] release version {__version__} is consistent")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
