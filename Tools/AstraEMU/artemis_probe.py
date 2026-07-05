from __future__ import annotations

import argparse
from collections import Counter

from common import dump_json, iter_files, magic_label, main_guard, parse_pf_archive, read_bytes


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe an Artemis game root and PFS patch chain.")
    ap.add_argument("root")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    packs = []
    ext = Counter()
    system_ini = []
    movies = []
    for path in iter_files(args.root):
        lower = path.name.lower()
        ext[path.suffix.lower() or "<none>"] += 1
        if lower.endswith(".pfs") or ".pfs." in lower:
            try:
                fmt, entries, _ = parse_pf_archive(path)
                packs.append({"path": str(path), "format": fmt, "entries": len(entries)})
                for e in entries:
                    if e["name"].lower().endswith("system.ini"):
                        system_ini.append({"archive": str(path), "entry": e["name"]})
            except Exception as exc:
                packs.append({"path": str(path), "error": str(exc)})
        elif path.suffix.lower() in (".wmv", ".dat"):
            head = read_bytes(path, 16)
            if magic_label(head) == "RIFF" or head.startswith(b"0&\xb2u"):
                movies.append(str(path))
    report = {"root": args.root, "packs": packs, "extension_counts": dict(ext), "system_ini": system_ini, "movies": movies}
    dump_json(report) if args.json else print(report)


if __name__ == "__main__":
    main_guard(main)
