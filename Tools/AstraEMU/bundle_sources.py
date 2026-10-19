#!/usr/bin/env python3
"""Bundle the corresponding source of a vcpkg-built LGPL dependency.

The ZIP holds the original upstream archives exactly as vcpkg downloaded them
(`downloads/<name>`) and the port directories that built them
(`ports/<port>/...`: portfile, manifest and patches). Nothing is extracted,
rebuilt or fetched.
"""

import argparse
import sys
import zipfile
from pathlib import Path


TIMESTAMP = (1980, 1, 1, 0, 0, 0)


def _entries(downloads: Path, archives: list[str], ports: list[Path]) -> list[tuple[Path, str]]:
    entries = []
    for pattern in archives:
        if "/" in pattern or "\\" in pattern:
            raise ValueError(f"archive pattern must be a file name: {pattern}")
        matches = sorted(path for path in downloads.glob(pattern) if path.is_file() and not path.is_symlink())
        if not matches:
            raise ValueError(f"no downloaded source archive matches {pattern}")
        entries.extend((path, "downloads/" + path.name) for path in matches)
    for port in ports:
        if port.is_symlink() or not (port / "portfile.cmake").is_file():
            raise ValueError(f"not a vcpkg port directory: {port}")
        for path in sorted(port.rglob("*")):
            if path.is_symlink():
                raise ValueError(f"linked port file: {path}")
            if path.is_file():
                entries.append((path, f"ports/{port.name}/{path.relative_to(port).as_posix()}"))
    names = [name for _, name in entries]
    if len(names) != len({name.casefold() for name in names}):
        raise ValueError("duplicate bundle entry")
    return entries


def bundle(downloads: Path, archives: list[str], ports: list[Path], output: Path) -> int:
    if output.exists() or output.is_symlink():
        raise ValueError("output must be a new file")
    entries = _entries(downloads, archives, ports)
    output.parent.mkdir(parents=True, exist_ok=True)
    partial = output.with_name(output.name + ".partial")
    try:
        with zipfile.ZipFile(partial, "x", zipfile.ZIP_DEFLATED) as archive:
            for source, name in entries:
                info = zipfile.ZipInfo(name, TIMESTAMP)
                info.compress_type = zipfile.ZIP_STORED if name.startswith("downloads/") else zipfile.ZIP_DEFLATED
                info.external_attr = 0o644 << 16
                with source.open("rb") as stream, archive.open(info, "w") as target:
                    while chunk := stream.read(1024 * 1024):
                        target.write(chunk)
        partial.rename(output)
    finally:
        partial.unlink(missing_ok=True)
    return len(entries)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--downloads", type=Path, required=True, help="vcpkg downloads root")
    parser.add_argument("--archive", action="append", required=True, help="file name glob in --downloads")
    parser.add_argument("--port", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        count = bundle(args.downloads, args.archive, args.port, args.output)
    except (OSError, ValueError) as error:
        print(f"AstraEMU source bundling failed: {error}", file=sys.stderr)
        return 1
    print(f"{args.output}: {count} file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
