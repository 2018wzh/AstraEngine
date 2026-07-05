from __future__ import annotations

import argparse
from collections import Counter

from common import dump_json, iter_files, magic_label, main_guard, parse_bgi_archive, read_bgi_entry


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe a BGI game root and count archive payload magic.")
    ap.add_argument("root")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    archives = []
    magic = Counter()
    entries_total = 0
    for path in iter_files(args.root, [".arc"]):
        try:
            fmt, entries = parse_bgi_archive(path)
        except Exception:
            continue
        entries_total += len(entries)
        local = Counter()
        for entry in entries:
            label = magic_label(read_bgi_entry(path, entry)[:32])
            local[label] += 1
            magic[label] += 1
        archives.append({"path": str(path), "format": fmt, "entries": len(entries), "magic": dict(local)})
    report = {"root": args.root, "archives": archives, "archive_count": len(archives), "entry_count": entries_total, "magic": dict(magic)}
    dump_json(report) if args.json else print(report)


if __name__ == "__main__":
    main_guard(main)
