#!/usr/bin/env python3
"""Collect and verify the app-local DLL closure of staged Family cores.

`check`: each dependency reported by `dumpbin /dependents` must be one of: a
file in the same core directory, a VC runtime DLL shipped at the package root,
an `api-ms-win-*` API set, or a System32 DLL that is not a redistributable MSVC
runtime. `closure`: print the DLLs a core library transitively needs from the
given build directories. Binaries are inspected, never loaded.
"""

import argparse
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path


PACKAGE_ROOT_RUNTIME = ("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll")
REDISTRIBUTABLE_PREFIXES = ("msvcp", "vcruntime", "vcomp", "concrt", "vccorlib")
SECTION_HEADERS = (
    "image has the following dependencies:",
    "image has the following delay load dependencies:",
)


def parse_dependents(text: str) -> list[str]:
    dependencies = []
    collecting = False
    for line in text.splitlines():
        stripped = line.strip()
        lowered = stripped.lower()
        if lowered in SECTION_HEADERS:
            collecting = True
            continue
        if not collecting or not stripped:
            continue
        if lowered.endswith(".dll") and line[:1].isspace():
            dependencies.append(stripped)
        else:
            collecting = False
    return dependencies


def run_dumpbin(dumpbin: str, path: Path) -> str:
    result = subprocess.run([dumpbin, "/nologo", "/dependents", str(path)],
                            capture_output=True, text=True, errors="replace")
    if result.returncode != 0:
        raise RuntimeError(f"dumpbin failed for {path}: {result.stdout}{result.stderr}")
    return result.stdout


def violations(
    core_dirs: list[Path], *, system_dir: Path, dump: Callable[[Path], str],
    root_runtime: tuple[str, ...] = PACKAGE_ROOT_RUNTIME,
) -> list[str]:
    allowed_root = {name.casefold() for name in root_runtime}
    problems = []
    for directory in core_dirs:
        local = {path.name.casefold() for path in directory.iterdir() if path.is_file()}
        binaries = sorted(path for path in directory.rglob("*") if path.is_file() and path.suffix.lower() == ".dll")
        if not binaries:
            problems.append(f"{directory}: no DLL found")
        for binary in binaries:
            for dependency in parse_dependents(dump(binary)):
                name = dependency.casefold()
                if name in local or name in allowed_root or name.startswith("api-ms-win-"):
                    continue
                if not name.startswith(REDISTRIBUTABLE_PREFIXES) and (system_dir / dependency).is_file():
                    continue
                problems.append(f"{binary.relative_to(directory.parent)}: unresolved dependency {dependency}")
    return problems


def closure(
    library: Path, search_dirs: list[Path], *, dump: Callable[[Path], str],
    root_runtime: tuple[str, ...] = PACKAGE_ROOT_RUNTIME,
) -> list[Path]:
    """App-local DLLs `library` transitively needs, taken from `search_dirs` in order."""
    skipped = {name.casefold() for name in root_runtime}
    found: dict[str, Path] = {}
    pending = [library]
    visited = {library.name.casefold()}
    while pending:
        for dependency in parse_dependents(dump(pending.pop())):
            name = dependency.casefold()
            if name in visited or name in skipped or name.startswith("api-ms-win-"):
                continue
            visited.add(name)
            match = next((directory / dependency for directory in search_dirs
                          if (directory / dependency).is_file()), None)
            if match is None:
                continue
            found[name] = match
            pending.append(match)
    return sorted(found.values(), key=lambda path: path.name.casefold())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--dumpbin", default="dumpbin")
    parser.add_argument("--system-dir", type=Path, default=Path(r"C:\Windows\System32"))
    commands = parser.add_subparsers(dest="command", required=True)
    check = commands.add_parser("check", help="verify staged core directories", allow_abbrev=False)
    check.add_argument("--core-dir", type=Path, action="append", required=True)
    collect = commands.add_parser("closure", help="print app-local dependencies of a library",
                                  allow_abbrev=False)
    collect.add_argument("--library", type=Path, required=True)
    collect.add_argument("--search-dir", type=Path, action="append", default=[])
    args = parser.parse_args()
    dump = lambda path: run_dumpbin(args.dumpbin, path)
    try:
        if args.command == "closure":
            for path in closure(args.library, args.search_dir, dump=dump):
                print(path)
            return 0
        problems = violations(args.core_dir, system_dir=args.system_dir, dump=dump)
    except (OSError, RuntimeError) as error:
        print(f"AstraEMU core dependency check failed: {error}", file=sys.stderr)
        return 1
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        return 1
    print(f"Core dependencies verified for {len(args.core_dir)} core directory(ies).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
