#!/usr/bin/env python3
"""Build release core archives and their AstraEMU-metadata catalog entries.

`archive`: zip each staged core directory (contents at the archive root, the
layout extracted into `cores/.pending/<stem>/`) as
`astra-emu-core-<stem>-<version>-<platform>.zip`.
`catalog`: for each core archive write `<plugin_id>.json` (a cores.v1
CoreCatalogEntry) and a release-notes list of core versions.
`crate-version`: print a Cargo package version, resolving `version.workspace`.
"""

import argparse
import hashlib
import json
import re
import sys
import tomllib
import zipfile
from pathlib import Path


PLATFORM = "windows-x86_64"
CORE_PACKAGE_SCHEMA = "astra.emu.core_package.v1"
TIMESTAMP = (1980, 1, 1, 0, 0, 0)


def crate_version(manifest: Path) -> str:
    package = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]
    version = package["version"]
    if isinstance(version, str):
        return version
    if version != {"workspace": True}:
        raise ValueError(f"unsupported version field in {manifest}")
    for directory in manifest.resolve().parent.parents:
        candidate = directory / "Cargo.toml"
        if candidate.is_file():
            workspace = tomllib.loads(candidate.read_text(encoding="utf-8")).get("workspace", {})
            if "version" in workspace.get("package", {}):
                return workspace["package"]["version"]
    raise ValueError(f"no workspace version for {manifest}")


def archive_name(stem: str, version: str, platform: str = PLATFORM) -> str:
    return f"astra-emu-core-{stem}-{version}-{platform}.zip"


def write_archive(core_dir: Path, version: str, output_dir: Path) -> Path:
    if not (core_dir / "core.json").is_file():
        raise ValueError(f"{core_dir} is not a staged core directory")
    output = output_dir / archive_name(core_dir.name, version)
    if output.exists():
        raise ValueError(f"{output} already exists")
    output_dir.mkdir(parents=True, exist_ok=True)
    files = sorted((path for path in core_dir.rglob("*") if path.is_file()),
                   key=lambda path: path.relative_to(core_dir).as_posix())
    with zipfile.ZipFile(output, "x", zipfile.ZIP_DEFLATED) as archive:
        for path in files:
            if path.is_symlink():
                raise ValueError(f"linked core file: {path}")
            info = zipfile.ZipInfo(path.relative_to(core_dir).as_posix(), TIMESTAMP)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, path.read_bytes())
    return output


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def catalog_entries(archives: list[Path], table: dict, base_url: str, tag: str) -> list[dict]:
    if not base_url.startswith("https://") or not base_url.endswith("/"):
        raise ValueError("base URL must be https and end with '/'")
    entries, seen = [], set()
    for path in archives:
        with zipfile.ZipFile(path) as archive:
            manifest = json.loads(archive.read("core.json"))
        if manifest.get("schema") != CORE_PACKAGE_SCHEMA:
            raise ValueError(f"{path.name} lacks a core package manifest")
        plugin_id = manifest["plugin_id"]
        if plugin_id not in table:
            raise ValueError(f"{plugin_id} is missing from the release core table")
        if plugin_id in seen:
            raise ValueError(f"duplicate core archive for {plugin_id}")
        seen.add(plugin_id)
        if not re.fullmatch(r"[A-Za-z0-9._+-]+\.zip", path.name):
            raise ValueError(f"invalid archive name {path.name}")
        info = table[plugin_id]
        entries.append({
            "plugin_id": plugin_id,
            "display_name": info["display_name"],
            "family_name": info["family_name"],
            "version": manifest["version"],
            "abi": manifest["abi"],
            "platform": PLATFORM,
            "url": base_url + path.name,
            "sha256": _sha256(path),
            "size": path.stat().st_size,
            "license": info["license"],
            "release_tag": tag,
        })
    return sorted(entries, key=lambda entry: entry["plugin_id"])


def release_notes(entries: list[dict], tag: str) -> str:
    lines = [f"AstraEMU {tag} 核心版本：", ""]
    lines += [f"- {entry['display_name']}（{entry['family_name']}，`{entry['plugin_id']}`）{entry['version']}"
              for entry in entries]
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    commands = parser.add_subparsers(dest="command", required=True)
    pack = commands.add_parser("archive", allow_abbrev=False)
    pack.add_argument("--core-dir", type=Path, action="append", required=True)
    pack.add_argument("--version", required=True)
    pack.add_argument("--output-dir", type=Path, required=True)
    describe = commands.add_parser("catalog", allow_abbrev=False)
    describe.add_argument("--archive", type=Path, action="append", required=True)
    describe.add_argument("--table", type=Path, required=True)
    describe.add_argument("--base-url", required=True)
    describe.add_argument("--tag", required=True)
    describe.add_argument("--output-dir", type=Path, required=True)
    describe.add_argument("--notes", type=Path, required=True)
    version = commands.add_parser("crate-version", allow_abbrev=False)
    version.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "crate-version":
            print(crate_version(args.manifest))
            return 0
        if args.command == "archive":
            for core_dir in args.core_dir:
                print(write_archive(core_dir, args.version, args.output_dir))
            return 0
        table = json.loads(args.table.read_text(encoding="utf-8"))
        entries = catalog_entries(args.archive, table, args.base_url, args.tag)
        args.output_dir.mkdir(parents=True, exist_ok=True)
        for entry in entries:
            (args.output_dir / f"{entry['plugin_id']}.json").write_text(
                json.dumps(entry, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        args.notes.write_text(release_notes(entries, args.tag), encoding="utf-8")
    except (OSError, ValueError, KeyError, zipfile.BadZipFile, tomllib.TOMLDecodeError) as error:
        print(f"AstraEMU release catalog failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
