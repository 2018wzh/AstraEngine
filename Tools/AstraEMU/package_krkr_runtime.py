#!/usr/bin/env python3
"""Assemble the native KrKr C ABI runtime directory.

The native owner's staged runtime contains astra_emu_krkr.dll, its app-local
dependencies and Resources. No child executable or Rust forwarding DLL is used.
This copies real build outputs; it does not build, launch, register or publish.
The output must be new so stale DLLs cannot survive an incremental build.
"""

import argparse
import shutil
import struct
import tempfile
from pathlib import Path


class PackageError(ValueError):
    pass


def check_pe(path: Path, *, dll: bool) -> None:
    """Read only the bounded PE header; no library load or native execution."""
    with path.open("rb") as stream:
        dos = stream.read(64)
        if len(dos) != 64 or dos[:2] != b"MZ":
            raise PackageError("runtime artifact is not a PE image")
        offset = struct.unpack_from("<I", dos, 60)[0]
        if not 64 <= offset <= 1024 * 1024:
            raise PackageError("runtime PE header offset is invalid")
        stream.seek(offset)
        header = stream.read(26)
    if len(header) != 26 or header[:4] != b"PE\0\0":
        raise PackageError("runtime PE header is invalid")
    machine, = struct.unpack_from("<H", header, 4)
    characteristics, = struct.unpack_from("<H", header, 22)
    magic, = struct.unpack_from("<H", header, 24)
    if machine != 0x8664 or magic != 0x20B or bool(characteristics & 0x2000) != dll:
        raise PackageError("runtime artifact must be a Windows x64 image of the expected kind")


def resource_files(root: Path) -> list[Path]:
    files = []
    if not root.is_dir() or root.is_symlink():
        raise PackageError("native Resources directory is missing or linked")
    for path in root.rglob("*"):
        if path.is_symlink() or not path.resolve().is_relative_to(root):
            raise PackageError("native resource links are not supported")
        if path.is_file():
            files.append(path)
    if not files:
        raise PackageError("native Resources directory is empty")
    return files


def package(native_runtime: Path, output: Path, runtime_files: list[Path]) -> int:
    native_runtime = native_runtime.resolve(strict=True)
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise PackageError("output directory must be new")
    output.parent.mkdir(parents=True, exist_ok=True)
    output = output.parent.resolve() / output.name
    if output.is_relative_to(native_runtime):
        raise PackageError("output directory must be separate from build inputs")

    sources = [path for path in native_runtime.iterdir() if path.suffix.casefold() == ".dll"]
    if not any(path.name == "astra_emu_krkr.dll" for path in sources):
        raise PackageError("native C ABI core DLL is missing")
    sources.extend(path.resolve(strict=True) for path in runtime_files)
    names = set()
    for path in sources:
        if not path.is_file() or path.is_symlink() or path.suffix.casefold() != ".dll":
            raise PackageError("runtime build output is missing or linked")
        if path.name.casefold() in names:
            raise PackageError("runtime artifacts have a duplicate DLL name")
        names.add(path.name.casefold())
        check_pe(path, dll=True)
    if not {"msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll"}.issubset(names):
        raise PackageError("the native core requires app-local MSVC runtime DLLs")
    resources = native_runtime / "Resources"
    resource_files(resources)

    # Stage beside the final directory, then rename only after every copy.
    # TemporaryDirectory owns only the fresh staging container on failure.
    with tempfile.TemporaryDirectory(prefix=".krkr-runtime-", dir=output.parent) as temporary:
        stage = Path(temporary) / "runtime"
        stage.mkdir()
        for path in sources:
            shutil.copy2(path, stage / path.name)
        shutil.copytree(resources, stage / "Resources")
        if output.exists() or output.is_symlink():
            raise PackageError("output directory appeared during packaging")
        stage.rename(output)
    return len(sources) - 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-runtime", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runtime-file", type=Path, action="append", default=[])
    args = parser.parse_args()
    try:
        dependencies = package(args.native_runtime, args.output, args.runtime_file)
    except (OSError, PackageError):
        parser.exit(1, "KrKr runtime packaging failed: verify inputs and a new output directory.\n")
    print(f"KrKr runtime packaged with {dependencies} native dependency DLLs and Resources.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
