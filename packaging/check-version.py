#!/usr/bin/env python3
"""Checks that the release version agrees everywhere it is written.

The packages take their version from Cargo.toml; the AppStream metadata
lists releases by hand, newest first.

    check-version.py          Cargo.toml and the newest metainfo release agree
    check-version.py v1.2.3   ...and so does this release tag

Prints the version.
"""

import sys

if sys.version_info < (3, 11):
    sys.exit("check-version.py needs Python 3.11+ (tomllib); on macOS, Homebrew's python3")

import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
METAINFO = ROOT / "packaging/linux/io.github.fritojay.Lune.metainfo.xml"


def main() -> None:
    args = sys.argv[1:]
    if len(args) > 1:
        sys.exit(__doc__)
    with (ROOT / "Cargo.toml").open("rb") as f:
        version = tomllib.load(f)["workspace"]["package"]["version"]

    release = ET.parse(METAINFO).find("releases/release")
    newest = None if release is None else release.get("version")
    errors = []
    if newest != version:
        errors.append(f"{METAINFO.name}: newest release is {newest}, Cargo.toml has {version}")
    if args and args[0] != f"v{version}":
        errors.append(f"tag {args[0]} does not match Cargo.toml version {version} (expected v{version})")
    if errors:
        sys.exit("\n".join(errors))
    print(version)


if __name__ == "__main__":
    main()
