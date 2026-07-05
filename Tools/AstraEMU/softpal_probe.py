from __future__ import annotations

import argparse
from collections import Counter

from common import dump_json, iter_files, magic_label, main_guard, parse_softpal_pac, read_bytes


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe a SoftPAL game root.")
    ap.add_argument("root")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    pacs = []
    magic = Counter()
    for path in iter_files(args.root):
        magic[magic_label(read_bytes(path, 16))] += 1
        if path.suffix.lower() == ".pac":
            try:
                pacs.append({"path": str(path), "entries": len(parse_softpal_pac(path))})
            except Exception as exc:
                pacs.append({"path": str(path), "error": str(exc)})
    report = {"root": args.root, "pac": pacs, "magic": dict(magic)}
    dump_json(report) if args.json else print(report)


if __name__ == "__main__":
    main_guard(main)
