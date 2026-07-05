from __future__ import annotations

import argparse
from collections import Counter

from common import dump_json, iter_files, magic_label, main_guard, parse_fvp_bin, parse_fvp_hcb, read_bytes


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe an FVP game root for HCB, BIN, and media signatures.")
    ap.add_argument("root")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    hcb = []
    bins = []
    magic = Counter()
    for path in iter_files(args.root):
        suffix = path.suffix.lower()
        if suffix == ".hcb":
            hcb.append({"path": str(path), **parse_fvp_hcb(path)})
        elif suffix == ".bin":
            try:
                entries = parse_fvp_bin(path)
                bins.append({"path": str(path), "entries": len(entries)})
            except Exception:
                pass
        magic[magic_label(read_bytes(path, 16))] += 1
    report = {"root": args.root, "hcb": hcb, "bins": bins, "magic": dict(magic)}
    dump_json(report) if args.json else print(report)


if __name__ == "__main__":
    main_guard(main)
