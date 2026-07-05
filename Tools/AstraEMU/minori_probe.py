from __future__ import annotations

import argparse

from common import dump_json, iter_files, magic_label, main_guard, read_bytes


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe a Minori game root for PAZ, MYS, executables, and visible signatures.")
    ap.add_argument("root")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    files = []
    for path in iter_files(args.root):
        if path.suffix.lower() in (".paz", ".mys", ".exe", ".chm"):
            files.append({"path": str(path), "size": path.stat().st_size, "magic": magic_label(read_bytes(path, 16))})
    report = {"root": args.root, "files": files}
    dump_json(report) if args.json else print(report)


if __name__ == "__main__":
    main_guard(main)
