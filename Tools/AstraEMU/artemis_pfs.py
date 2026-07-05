from __future__ import annotations

import argparse

from common import dump_json, main_guard, parse_pf_archive, print_rows, read_pf_entry, require_out_dir, write_entry


def main() -> None:
    ap = argparse.ArgumentParser(description="List, read, or extract Artemis PF6/PF8 archives.")
    ap.add_argument("archive")
    ap.add_argument("--entry", help="entry name for read/extract")
    ap.add_argument("--out", help="extract all entries or selected --entry to this directory")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    fmt, entries, key = parse_pf_archive(args.archive)
    rows = [{"format": fmt, **e} for e in entries]
    if args.out:
        out = require_out_dir(args.out)
        selected = [e for e in entries if not args.entry or e["name"] == args.entry]
        written = [str(write_entry(out, e["name"], read_pf_entry(args.archive, e, key))) for e in selected]
        rows = [{"written": len(written), "files": written}]
    elif args.entry:
        entry = next((e for e in entries if e["name"] == args.entry), None)
        if entry is None:
            raise SystemExit(f"entry not found: {args.entry}")
        rows = [{"entry": args.entry, "size": len(read_pf_entry(args.archive, entry, key)), "format": fmt}]
    dump_json(rows) if args.json else print_rows(rows, ("format", "name", "offset", "size", "encrypted"))


if __name__ == "__main__":
    main_guard(main)
