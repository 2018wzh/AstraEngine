#!/usr/bin/env python3
"""Assemble a desktop AstraEMU client from real Manager and Family outputs."""

import argparse
import shutil
import sys
import tempfile
from pathlib import Path


def package(manager: Path, cores: list[Path], output: Path, runtime_files: list[Path]) -> None:
    manager = manager.resolve(strict=True)
    cores = [path.resolve(strict=True) for path in cores]
    runtime_files = [path.resolve(strict=True) for path in runtime_files]
    output = output.absolute()
    if not manager.is_file() or not cores:
        raise ValueError("a Manager executable and at least one Family are required")
    if output.exists() or output.is_symlink():
        raise ValueError("output must be a new directory")
    names = {
        manager.name.casefold(), "readme.md", "third_party_notices.md", "cores", "licenses"
    }
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
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".astraemu-client-", dir=output.parent) as temporary:
        stage = Path(temporary) / "client"
        stage.mkdir()
        shutil.copy2(manager, stage / manager.name)
        for runtime in runtime_files:
            shutil.copy2(runtime, stage / runtime.name)
        bundled = stage / "cores"
        bundled.mkdir()
        for core in cores:
            shutil.copy2(core, bundled / core.name)
        repo = Path(__file__).resolve().parents[2]
        shutil.copy2(repo / "Emulator/THIRD_PARTY_NOTICES.md", stage / "THIRD_PARTY_NOTICES.md")
        shutil.copy2(repo / "Docs/manual/astraemu-client.md", stage / "README.md")
        licenses = stage / "licenses"
        licenses.mkdir()
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
        if any("fvp" in core.stem for core in cores):
            destination = licenses / "wmv-decoder"
            destination.mkdir()
            shutil.copy2(
                repo / "Emulator/Source/Families/na_wmv_player/THIRD_PARTY_NOTICES.md",
                destination / "THIRD_PARTY_NOTICES.md",
            )
        stage.rename(output)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manager", type=Path, required=True)
    parser.add_argument("--core", type=Path, action="append", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runtime-file", type=Path, action="append", default=[])
    args = parser.parse_args()
    try:
        package(args.manager, args.core, args.output, args.runtime_file)
    except (OSError, ValueError):
        print("AstraEMU packaging failed: supply built artifacts and a new output directory.", file=sys.stderr)
        return 1
    print(f"AstraEMU client packaged with {len(args.core)} Family core(s).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
