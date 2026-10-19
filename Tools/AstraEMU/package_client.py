#!/usr/bin/env python3
"""Assemble a desktop AstraEMU client from real Manager, staged core and metadata outputs."""

import argparse
import ctypes
import hashlib
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
REPO = Path(__file__).resolve().parents[2]
UI_CRATE = REPO / "Emulator/Source/Manager/astra-emu-manager-ui-slint"
MATERIAL_LICENSE = UI_CRATE / "material-1.1.0/LICENSE.md"
SYMBOLS_LICENSE = UI_CRATE / "ui/icons/LICENSE"
CORE_PACKAGE_SCHEMA = "astra.emu.core_package.v1"
METADATA_INDEX_SCHEMA = "astra.emu.metadata.index.v1"
MAX_METADATA_FILE = 8 * 1024 * 1024
MAX_METADATA_TOTAL = 64 * 1024 * 1024


def _linked(path: Path) -> bool:
    return path.is_symlink() or bool(getattr(path.lstat(), "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def _plain_file(path: Path) -> Path:
    if _linked(path):
        raise ValueError("linked package inputs are not supported")
    path = path.resolve(strict=True)
    if not path.is_file():
        raise ValueError("package input must be a regular file")
    return path


def _plain_dir(path: Path) -> Path:
    if _linked(path):
        raise ValueError("linked package inputs are not supported")
    path = path.resolve(strict=True)
    if not path.is_dir():
        raise ValueError("package input must be a directory")
    return path


def _asset(root: Path, relative: str) -> Path:
    # Portable package names: no traversal, ADS, device names or hidden paths.
    parts = relative.split("/")
    for part in parts:
        if (
            not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.+-]*", part)
            or part.endswith(".")
            or part.split(".")[0].upper() in {"CON", "PRN", "AUX", "NUL"}
            or re.fullmatch(r"(COM|LPT)[1-9]", part.split(".")[0], re.I)
        ):
            raise ValueError("invalid package asset path")
    path = root
    for part in parts:
        path = path / part
        if _linked(path):
            raise ValueError("linked package assets are not supported")
    return _plain_file(path)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _core_files(directory: Path) -> tuple[str, list[tuple[Path, str]]]:
    """Validate a stage_core.py output directory and return its stem and files."""
    root = _plain_dir(directory)
    manifest_path = _asset(root, "core.json")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("schema") != CORE_PACKAGE_SCHEMA:
        raise ValueError("core directory lacks a core package manifest")
    for key in ("plugin_id", "library", "version", "abi"):
        if not isinstance(manifest.get(key), str) or not manifest[key]:
            raise ValueError("invalid core package manifest")
    library = manifest["library"]
    if library.rsplit(".", 1)[0] != root.name or not library.lower().startswith(("astra_emu_", "libastra_emu_")):
        raise ValueError("core directory name must be the library stem")
    if library.rsplit(".", 1)[-1].lower() not in ("dll", "so", "dylib"):
        raise ValueError("core must be a desktop dynamic library")
    records = manifest.get("files")
    if not isinstance(records, list):
        raise ValueError("invalid core package manifest")
    files = [(manifest_path, "core.json")]
    listed = set()
    for record in records:
        if not isinstance(record, dict) or set(record) != {"path", "sha256"}:
            raise ValueError("invalid core package file record")
        relative = record["path"]
        if not isinstance(relative, str) or relative.casefold() in listed or relative == "core.json":
            raise ValueError("duplicate core package file")
        listed.add(relative.casefold())
        source = _asset(root, relative)
        if _sha256(source) != record["sha256"]:
            raise ValueError("core package file hash mismatch")
        files.append((source, relative))
    if library.casefold() not in listed:
        raise ValueError("core package manifest omits its library")
    libraries = [name for _, name in files if "/" not in name and name.lower().startswith(("astra_emu_", "libastra_emu_"))]
    if libraries != [library]:
        raise ValueError("a core package holds exactly one Family library")
    present = set()
    for path in root.rglob("*"):
        if _linked(path):
            raise ValueError("linked core package files are not supported")
        if path.is_file():
            present.add(path.relative_to(root).as_posix().casefold())
    if present != listed | {"core.json"}:
        raise ValueError("core directory contains unlisted files")
    return root.name, files


def _metadata_files(root: Path) -> list[tuple[Path, str]]:
    """Validate a build_index.py output (the directory holding index.json)."""
    root = _plain_dir(root)
    index_path = _asset(root, "index.json")
    index = json.loads(index_path.read_text(encoding="utf-8"))
    if not isinstance(index, dict) or index.get("schema") != METADATA_INDEX_SCHEMA:
        raise ValueError("metadata dist lacks an index.json v1")
    records = index.get("files")
    if not isinstance(records, list):
        raise ValueError("invalid metadata index")
    files = [(index_path, "index.json")]
    names, total = {"index.json"}, 0
    for record in records:
        if not isinstance(record, dict) or not {"path", "sha256", "size"} <= set(record):
            raise ValueError("invalid metadata index entry")
        relative, size = record["path"], record["size"]
        if (
            not isinstance(relative, str)
            or not re.fullmatch(r"[a-z0-9._/-]+", relative)
            or ".." in relative.split("/")
            or relative.startswith("/")
            or relative in names
        ):
            raise ValueError("invalid metadata path")
        names.add(relative)
        if not isinstance(size, int) or isinstance(size, bool) or not 0 <= size <= MAX_METADATA_FILE:
            raise ValueError("metadata file exceeds the size limit")
        total += size
        if total > MAX_METADATA_TOTAL:
            raise ValueError("metadata exceeds the total size limit")
        source = _asset(root, relative)
        if source.stat().st_size != size or _sha256(source) != record["sha256"]:
            raise ValueError("metadata file hash mismatch")
        files.append((source, relative))
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
    manager: Path, core_dirs: list[Path], output: Path, runtime_files: list[Path], *,
    metadata_dist: Path | None = None, luau_license: Path | None = None,
    subtitle_share: Path | None = None, material_license: Path = MATERIAL_LICENSE,
    symbols_license: Path = SYMBOLS_LICENSE,
) -> None:
    manager = _plain_file(manager)
    runtime_files = [_plain_file(path) for path in runtime_files]
    if not core_dirs:
        raise ValueError("a Manager executable and at least one Family core are required")
    if output.exists() or output.is_symlink():
        raise ValueError("output must be a new directory")
    output = output.resolve()
    names = {"readme.md", "third_party_notices.md", "cores", "licenses", "metadata",
             "astra-emu-patch", "astra-emu-patch.exe"}
    if manager.name.casefold() in names:
        raise ValueError("Manager filename collides with package contents")
    names.add(manager.name.casefold())
    cores = []
    stems = set()
    for directory in core_dirs:
        stem, files = _core_files(directory)
        if stem.casefold() in stems:
            raise ValueError("duplicate core directory")
        stems.add(stem.casefold())
        cores.append((stem, files))
    for runtime in runtime_files:
        if not runtime.is_file() or runtime.name.casefold() in names:
            raise ValueError("missing or duplicate runtime file")
        names.add(runtime.name.casefold())
    metadata = _metadata_files(metadata_dist) if metadata_dist is not None else []
    license_files = [
        (_plain_file(REPO / "Emulator/ThirdParty/mlua/LICENSE"), "mlua/LICENSE"),
        (_luau_license(luau_license), "luau/LICENSE"),
        (_plain_file(material_license), "slint-material/LICENSE.md"),
        (_plain_file(symbols_license), "material-symbols/LICENSE"),
    ]
    if subtitle_share is not None:
        for port in SUBTITLE_PORTS:
            license_files.append((_asset(subtitle_share, f"{port}/copyright"), f"subtitle/{port}/copyright"))
    sources = [manager, *runtime_files, *(source for _, files in cores for source, _ in files),
               *(source for source, _ in metadata), *(source for source, _ in license_files)]
    if any(source.is_relative_to(output) for source in sources):
        raise ValueError("output overlaps package inputs")
    inputs = [Path(directory).resolve() for directory in core_dirs]
    if metadata_dist is not None:
        inputs.append(Path(metadata_dist).resolve())
    if any(output.is_relative_to(directory) for directory in inputs):
        raise ValueError("output overlaps a package input directory")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".astraemu-client-", dir=output.parent) as temporary:
        stage = Path(temporary) / "client"
        stage.mkdir()
        shutil.copy2(manager, stage / manager.name)
        for runtime in runtime_files:
            shutil.copy2(runtime, stage / runtime.name)
        for source, relative in metadata:
            destination = stage / "metadata" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        bundled = stage / "cores"
        bundled.mkdir()
        for stem, files in cores:
            for source, relative in files:
                destination = bundled / stem / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, destination)
        shutil.copy2(REPO / "Emulator/THIRD_PARTY_NOTICES.md", stage / "THIRD_PARTY_NOTICES.md")
        shutil.copy2(REPO / "Docs/manual/astraemu-client.md", stage / "README.md")
        licenses = stage / "licenses"
        licenses.mkdir()
        for source, relative in license_files:
            destination = licenses / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        core_stems = [stem for stem, _ in cores]
        for family in (REPO / "Emulator/Source/Families").glob("astra-emu-*"):
            library_name = family.name.replace("-", "_")
            if not any(stem in (library_name, "lib" + library_name) for stem in core_stems):
                continue
            for name in ("THIRD_PARTY_NOTICES.md", "MODIFICATIONS.md", "LICENSE", "LICENSE-MPL-2.0"):
                source = family / name
                if source.is_file():
                    destination = licenses / family.name
                    destination.mkdir(exist_ok=True)
                    shutil.copy2(source, destination / name)
        for name in ("LICENSE", "LICENSE-MPL-2.0"):
            source = REPO / "Emulator/ThirdParty/rfvp" / name
            if source.is_file() and any("fvp" in stem for stem in core_stems):
                destination = licenses / "rfvp"
                destination.mkdir(exist_ok=True)
                shutil.copy2(source, destination / name)
        if any("musica" in stem for stem in core_stems):
            destination = licenses / "noto"
            destination.mkdir()
            shutil.copy2(
                REPO / "Examples/NativeVN/Assets/Fonts/OFL-NotoSansJP.txt",
                destination / "OFL.txt",
            )
        _publish(stage, output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--manager", type=Path, required=True)
    parser.add_argument("--core-dir", type=Path, action="append", required=True,
                        help="stage_core.py output directory <stem>/ holding core.json")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runtime-file", type=Path, action="append", default=[])
    parser.add_argument("--metadata-dist", type=Path, help="build_index.py output directory holding index.json")
    parser.add_argument("--luau-license", type=Path, help="LICENSE from the locked luau0-src source")
    parser.add_argument("--subtitle-share", type=Path, help="actual subtitle vcpkg installed/<triplet>/share")
    parser.add_argument("--material-license", type=Path, default=MATERIAL_LICENSE)
    parser.add_argument("--symbols-license", type=Path, default=SYMBOLS_LICENSE)
    args = parser.parse_args()
    try:
        package(
            args.manager, args.core_dir, args.output, args.runtime_file,
            metadata_dist=args.metadata_dist, luau_license=args.luau_license,
            subtitle_share=args.subtitle_share, material_license=args.material_license,
            symbols_license=args.symbols_license,
        )
    except (OSError, ValueError, AttributeError, KeyError, TypeError) as error:
        print(f"AstraEMU packaging failed: {error}", file=sys.stderr)
        return 1
    print(f"AstraEMU client packaged with {len(args.core_dir)} Family core(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
