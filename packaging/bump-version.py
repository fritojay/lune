#!/usr/bin/env python3
"""Sets the release version everywhere it is written.

    bump-version.py 1.2.3

Updates the workspace version in Cargo.toml and Cargo.lock, and adds a
release dated today at the top of the AppStream metadata. Then commit and
tag v1.2.3. check-version.py verifies the result.
"""

import datetime
import re
import subprocess
import sys

if sys.version_info < (3, 11):
    sys.exit("bump-version.py needs Python 3.11+ (tomllib); on macOS, Homebrew's python3")

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
METAINFO = ROOT / "packaging/linux/io.github.fritojay.Lune.metainfo.xml"


def parse(version: str) -> tuple[int, int, int]:
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not match:
        sys.exit(f"{version}: expected MAJOR.MINOR.PATCH, without a leading v")
    return tuple(int(part) for part in match.groups())


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    version = sys.argv[1]
    with CARGO_TOML.open("rb") as f:
        current = tomllib.load(f)["workspace"]["package"]["version"]
    if parse(version) <= parse(current):
        sys.exit(f"{version} is not newer than the current version {current}")

    # Edited as text, not re-serialized, so formatting and comments stay.
    toml = CARGO_TOML.read_text()
    toml, count = re.subn(
        r'(\[workspace\.package\][^\[]*?^version = )"[^"]*"',
        rf'\g<1>"{version}"',
        toml,
        count=1,
        flags=re.MULTILINE | re.DOTALL,
    )
    if count != 1:
        sys.exit(f"{CARGO_TOML.name}: no version line in [workspace.package]")

    metainfo = METAINFO.read_text()
    if f'<release version="{version}"' in metainfo:
        sys.exit(f"{METAINFO.name} already lists release {version}")
    today = datetime.date.today().isoformat()
    metainfo, count = re.subn(
        r"^(\s*)<release ",
        rf'\g<1><release version="{version}" date="{today}"/>\n\g<1><release ',
        metainfo,
        count=1,
        flags=re.MULTILINE,
    )
    if count != 1:
        sys.exit(f"{METAINFO.name}: no <release> to add the new one above")

    CARGO_TOML.write_text(toml)
    METAINFO.write_text(metainfo)
    # Rewrites only the workspace crates' entries in Cargo.lock.
    subprocess.run(["cargo", "update", "--workspace", "--offline"], cwd=ROOT, check=True)
    subprocess.run([ROOT / "packaging/check-version.py", f"v{version}"], check=True)


if __name__ == "__main__":
    main()
