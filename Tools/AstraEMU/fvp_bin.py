from __future__ import annotations

import argparse

from common import dump_json, main_guard, parse_fvp_bin, print_rows, read_bytes, require_out_dir, write_entry


def main() -> None:
    ap = argparse.ArgumentParser(description="List or extract FVP .bin package entries.")
    ap.add_argument("bin")
    ap.add_argument("--out", help="extract entries to this directory")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    entries = parse_fvp_bin(args.bin)
    rows = entries
    if args.out:
        data = read_bytes(args.bin)
        out = require_out_dir(args.out)
        written = [str(write_entry(out, e["name"], data[e["offset"] : e["offset"] + e["size"]])) for e in entries]
        rows = [{"written": len(written), "files": written}]
    dump_json(rows) if args.json else print_rows(rows, ("name", "offset", "size"))


if __name__ == "__main__":
    main_guard(main)
