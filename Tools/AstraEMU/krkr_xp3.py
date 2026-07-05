from __future__ import annotations

import argparse

from common import dump_json, main_guard, parse_xp3, print_rows, read_bytes, require_out_dir, write_entry


def main() -> None:
    ap = argparse.ArgumentParser(description="List or extract plain KrKr XP3 entries.")
    ap.add_argument("xp3")
    ap.add_argument("--out", help="extract all segments to this directory")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    entries = parse_xp3(args.xp3)
    rows = [{"name": e["name"], "size": e.get("size"), "segments": len(e.get("segments", []))} for e in entries]
    if args.out:
        data = read_bytes(args.xp3)
        out = require_out_dir(args.out)
        written = []
        for e in entries:
            if len(e.get("segments", [])) != 1 or e["segments"][0]["flags"] != 0:
                continue
            seg = e["segments"][0]
            payload = data[seg["offset"] : seg["offset"] + seg["compressed_size"]]
            written.append(str(write_entry(out, e["name"], payload)))
        rows = [{"written_plain_entries": len(written), "files": written}]
    dump_json(rows) if args.json else print_rows(rows, ("name", "size", "segments"))


if __name__ == "__main__":
    main_guard(main)
