#!/usr/bin/env python3
"""Assemble a desktop AstraEMU client from real Manager and Family outputs."""

import argparse
import ctypes
import json
import os
import re
import shutil
import stat
import sys
import tempfile
from pathlib import Path


SUBTITLE_PORTS = (
    "libass", "freetype", "fribidi", "harfbuzz", "brotli", "bzip2", "libpng", "zlib",
)
LUAU_SOURCE = "luau0-src-0.12.3+luau663"


def _plain_file(path: Path) -> Path:
    if path.is_symlink() or getattr(path.lstat(), "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
        raise ValueError("linked package inputs are not supported")
    path = path.resolve(strict=True)
    if not path.is_file():
        raise ValueError("package input must be a regular file")
    return path


def _asset(root: Path, relative: str) -> Path:
    # Portable package names: no traversal, ADS, device names or hidden paths.
    parts = relative.split("/")
    for part in parts:
        if (
            not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]*", part)
            or part.endswith(".")
            or part.split(".")[0].upper() in {"CON", "PRN", "AUX", "NUL"}
            or re.fullmatch(r"(COM|LPT)[1-9]", part.split(".")[0], re.I)
        ):
            raise ValueError("invalid package asset path")
    path = root
    for part in parts:
        path = path / part
        if path.is_symlink() or getattr(path.lstat(), "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
            raise ValueError("linked package assets are not supported")
    return _plain_file(path)


def _patch_files(root: Path, common: list[str]) -> list[tuple[Path, str]]:
    root = root.absolute()
    if root.is_symlink() or getattr(root.lstat(), "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
        raise ValueError("patch asset root must not be linked")
    catalog = _asset(root, "catalog.json")
    records = json.loads(catalog.read_text(encoding="utf-8"))
    if not isinstance(records, list):
        raise ValueError("patch catalog must be an array")
    files = [(catalog, "catalog.json")]
    ids, names = set(), set()
    for record in records:
        if not isinstance(record, dict) or set(record) != {"id", "title", "script"}:
            raise ValueError("invalid patch catalog entry")
        if any(not isinstance(record[key], str) or not record[key].strip() or "\0" in record[key]
               for key in record):
            raise ValueError("invalid patch catalog value")
        if record["id"] in ids:
            raise ValueError("duplicate patch catalog identity")
        ids.add(record["id"])
        name = record["script"]
        if "/" in name or not name.endswith(".luau"):
            raise ValueError("catalog entries must be top-level Luau files")
        files.append((_asset(root, name), name))
    for name in common:
        if not name.startswith("common/") or name.count("/") != 1 or not name.endswith(".luau"):
            raise ValueError("shared patch modules must be common/*.luau")
        files.append((_asset(root, name), name))
    for _, name in files:
        if re.search(r"(?:^|[/_.-])(test|tests|fixture|fixtures|scaffold|scaffolds)(?:[/_.-]|$)", name, re.I):
            raise ValueError("test scaffolds are not shipping patch assets")
        if name.casefold() in names:
            raise ValueError("duplicate patch asset path")
        names.add(name.casefold())
    return files


def _luau_license(explicit: Path | None) -> Path:
    if explicit is not None:
        return _plain_file(explicit)
    cargo = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    for source in sorted((cargo / "registry/src").glob(f"*/{LUAU_SOURCE}/LICENSE")):
        return _plain_file(source)
    raise ValueError(f"missing {LUAU_SOURCE}/LICENSE; supply --luau-license from the locked source")


def _publish(stage: Path, output: Path) -> None:
    """Publish without replacing even an empty directory created during copying."""
    if os.name == "nt":
        stage.rename(output)  # MoveFileEx without REPLACE_EXISTING.
        return
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform.startswith("linux"):
        rename = libc.renameat2
        rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
        args = (-100, os.fsencode(stage), -100, os.fsencode(output), 1)  # RENAME_NOREPLACE
    elif sys.platform == "darwin":
        rename = libc.renamex_np
        rename.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
        args = (os.fsencode(stage), os.fsencode(output), 4)  # RENAME_EXCL
    else:
        raise ValueError("no no-replace package publication primitive on this platform")
    rename.restype = ctypes.c_int
    if rename(*args):
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))


def package(
    manager: Path, cores: list[Path], output: Path, runtime_files: list[Path],
    patches: Path | None = None, *,
    patch_common: list[str] | None = None, luau_license: Path | None = None,
    subtitle_share: Path | None = None,
) -> None:
    manager = _plain_file(manager)
    cores = [_plain_file(path) for path in cores]
    runtime_files = [_plain_file(path) for path in runtime_files]
    if not cores:
        raise ValueError("a Manager executable and at least one Family are required")
    if output.exists() or output.is_symlink():
        raise ValueError("output must be a new directory")
    output = output.resolve()
    names = {"readme.md", "third_party_notices.md", "cores", "licenses", "patches",
             "astra-emu-patch", "astra-emu-patch.exe"}
    if manager.name.casefold() in names:
        raise ValueError("Manager filename collides with package contents")
    names.add(manager.name.casefold())
    if patches is None and patch_common:
        raise ValueError("shared patch modules require --patches")
    assets = _patch_files(patches, patch_common or []) if patches is not None else []
    repo = Path(__file__).resolve().parents[2]
    license_files = [
        (_plain_file(repo / "Emulator/ThirdParty/mlua/LICENSE"), "mlua/LICENSE"),
        (_luau_license(luau_license), "luau/LICENSE"),
    ]
    if subtitle_share is not None:
        for port in SUBTITLE_PORTS:
            license_files.append((_asset(subtitle_share, f"{port}/copyright"), f"subtitle/{port}/copyright"))
    for core in cores:
        if not core.is_file() or not core.name.startswith(("astra_emu_", "libastra_emu_")):
            raise ValueError("core must be a built Family dynamic library")
        if core.suffix.lower() not in (".dll", ".so", ".dylib"):
            raise ValueError("core must be a desktop dynamic library")
        if core.name.casefold() in names:
            raise ValueError("duplicate runtime filename")
        names.add(core.name.casefold())
    for runtime in runtime_files:
        if not runtime.is_file() or runtime.name.casefold() in names:
            raise ValueError("missing or duplicate runtime file")
        names.add(runtime.name.casefold())
    sources = [manager, *cores, *runtime_files, *(source for source, _ in assets),
               *(source for source, _ in license_files)]
    if any(source.is_relative_to(output) for source in sources):
        raise ValueError("output overlaps package inputs")
    if patches is not None and output.is_relative_to(patches.resolve()):
        raise ValueError("output overlaps the patch asset tree")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".astraemu-client-", dir=output.parent) as temporary:
        stage = Path(temporary) / "client"
        stage.mkdir()
        shutil.copy2(manager, stage / manager.name)
        for runtime in runtime_files:
            shutil.copy2(runtime, stage / runtime.name)
        for source, relative in assets:
            destination = stage / "patches" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        bundled = stage / "cores"
        bundled.mkdir()
        for core in cores:
            shutil.copy2(core, bundled / core.name)
        shutil.copy2(repo / "Emulator/THIRD_PARTY_NOTICES.md", stage / "THIRD_PARTY_NOTICES.md")
        shutil.copy2(repo / "Docs/manual/astraemu-client.md", stage / "README.md")
        licenses = stage / "licenses"
        licenses.mkdir()
        for source, relative in license_files:
            destination = licenses / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        for family in (repo / "Emulator/Source/Families").glob("astra-emu-*"):
            library_name = family.name.replace("-", "_")
            if not any(core.stem in (library_name, "lib" + library_name) for core in cores):
                continue
            for name in ("THIRD_PARTY_NOTICES.md", "MODIFICATIONS.md", "LICENSE", "LICENSE-MPL-2.0"):
                source = family / name
                if source.is_file():
                    destination = licenses / family.name
                    destination.mkdir(exist_ok=True)
                    shutil.copy2(source, destination / name)
        for name in ("LICENSE", "LICENSE-MPL-2.0"):
            source = repo / "Emulator/ThirdParty/rfvp" / name
            if source.is_file() and any("fvp" in core.stem for core in cores):
                destination = licenses / "rfvp"
                destination.mkdir(exist_ok=True)
                shutil.copy2(source, destination / name)
        if any("musica" in core.stem for core in cores):
            destination = licenses / "noto"
            destination.mkdir()
            shutil.copy2(
                repo / "Examples/NativeVN/Assets/Fonts/OFL-NotoSansJP.txt",
                destination / "OFL.txt",
            )
        _publish(stage, output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manager", type=Path, required=True)
    parser.add_argument("--core", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runtime-file", type=Path, action="append", default=[])
    parser.add_argument("--patches", type=Path, help="explicit trusted public patch catalog directory")
    parser.add_argument("--patch-common", action="append", default=[], help="required common/<module>.luau only")
    parser.add_argument("--luau-license", type=Path, help="LICENSE from the locked luau0-src source")
    parser.add_argument("--subtitle-share", type=Path, help="actual static subtitle vcpkg installed/<triplet>/share")
    args = parser.parse_args()
    try:
        package(
            args.manager, args.core, args.output, args.runtime_file,
            args.patches, patch_common=args.patch_common,
            luau_license=args.luau_license, subtitle_share=args.subtitle_share,
        )
    except (OSError, ValueError, AttributeError):
        print("AstraEMU packaging failed: verify built artifacts, licenses, trusted assets and a new output directory.", file=sys.stderr)
        return 1
    print(f"AstraEMU client packaged with {len(args.core)} Family core(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
