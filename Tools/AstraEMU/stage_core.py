#!/usr/bin/env python3
"""Stage one built Family core into a self-contained core package directory.

The result is `<output>/<stem>/` holding the core library, its dependencies,
optional `Resources/`, `licenses/` and a `core.json` manifest with the SHA-256
of every file. Files are copied, never executed. The output must be new.
"""

import argparse
import hashlib
import json
import re
import shutil
import stat
import sys
import tempfile
from pathlib import Path


CORE_PACKAGE_SCHEMA = "astra.emu.core_package.v1"
CORE_ABI = "astra.emu.native.c.v3"
LIBRARY_SUFFIXES = (".dll", ".so", ".dylib")
RESERVED = {"core.json", "licenses", "resources"}


class StageError(ValueError):
    pass


def _linked(path: Path) -> bool:
    return path.is_symlink() or bool(getattr(path.lstat(), "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def _plain_file(path: Path) -> Path:
    if _linked(path):
        raise StageError(f"linked input is not supported: {path}")
    path = path.resolve(strict=True)
    if not path.is_file():
        raise StageError(f"input must be a regular file: {path}")
    return path


def _portable(relative: str) -> str:
    parts = relative.split("/")
    for part in parts:
        if (
            not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.+-]*", part)
            or part.endswith(".")
            or part.split(".")[0].upper() in {"CON", "PRN", "AUX", "NUL"}
            or re.fullmatch(r"(COM|LPT)[1-9]", part.split(".")[0], re.I)
        ):
            raise StageError(f"invalid package path: {relative}")
    return relative


def is_family_library(name: str) -> bool:
    lowered = name.lower()
    return lowered.startswith(("astra_emu_", "libastra_emu_")) and lowered.endswith(LIBRARY_SUFFIXES)


def library_stem(name: str) -> str:
    return name.rsplit(".", 1)[0]


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _resource_files(root: Path) -> list[tuple[Path, str]]:
    if _linked(root) or not root.is_dir():
        raise StageError("resources must be a regular directory")
    root = root.resolve()
    files = []
    for path in sorted(root.rglob("*")):
        if _linked(path) or not path.resolve().is_relative_to(root):
            raise StageError("linked resources are not supported")
        if path.is_file():
            files.append((path, _portable("Resources/" + path.relative_to(root).as_posix())))
    if not files:
        raise StageError("resources directory is empty")
    return files


def _license_target(argument: str) -> tuple[Path, str]:
    name, separator, source = argument.partition("=")
    if separator and name and not any(character in name for character in ":\\"):
        return _plain_file(Path(source)), _portable("licenses/" + name)
    path = _plain_file(Path(argument))
    return path, _portable("licenses/" + path.name)


def stage(
    library: Path, plugin_id: str, version: str, output: Path, *,
    dependencies: list[Path] = (), resources: Path | None = None, licenses: list[str] = (),
) -> Path:
    library = _plain_file(library)
    if not is_family_library(library.name):
        raise StageError("library must be a built astra_emu_* Family dynamic library")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", plugin_id):
        raise StageError("invalid plugin id")
    if not re.fullmatch(r"[0-9A-Za-z][0-9A-Za-z.+-]{0,63}", version):
        raise StageError("invalid version")
    if output.exists() or output.is_symlink():
        raise StageError("output must be a new directory")
    stem = library_stem(library.name)
    entries: list[tuple[Path, str]] = [(library, _portable(library.name))]
    for dependency in dependencies:
        dependency = _plain_file(dependency)
        if is_family_library(dependency.name):
            raise StageError("a core package holds exactly one Family library")
        if dependency.name.casefold() in RESERVED:
            raise StageError("dependency name collides with package layout")
        entries.append((dependency, _portable(dependency.name)))
    if resources is not None:
        entries.extend(_resource_files(resources))
    entries.extend(_license_target(argument) for argument in licenses)
    seen = set()
    for _, relative in entries:
        if relative.casefold() in seen:
            raise StageError(f"duplicate package path: {relative}")
        seen.add(relative.casefold())
    output = output.absolute()
    if any(source.is_relative_to(output.resolve()) for source, _ in entries):
        raise StageError("output overlaps inputs")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".astraemu-core-", dir=output.parent) as temporary:
        root = Path(temporary) / "out"
        target = root / stem
        target.mkdir(parents=True)
        files = []
        for source, relative in entries:
            destination = target / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
            files.append({"path": relative, "sha256": sha256(destination)})
        manifest = {
            "schema": CORE_PACKAGE_SCHEMA,
            "plugin_id": plugin_id,
            "library": library.name,
            "version": version,
            "abi": CORE_ABI,
            "files": sorted(files, key=lambda item: item["path"]),
        }
        (target / "core.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        if output.exists() or output.is_symlink():
            raise StageError("output appeared during staging")
        root.rename(output)
    return output / stem


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--plugin-id", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--dependency", type=Path, action="append", default=[])
    parser.add_argument("--resources", type=Path)
    parser.add_argument("--license", action="append", default=[], help="<file> or <name>=<file>")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        staged = stage(
            args.library, args.plugin_id, args.version, args.output,
            dependencies=args.dependency, resources=args.resources, licenses=args.license,
        )
    except (OSError, StageError) as error:
        print(f"AstraEMU core staging failed: {error}", file=sys.stderr)
        return 1
    print(staged)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
