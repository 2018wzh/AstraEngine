from __future__ import annotations

import argparse

from common import (
    add_json_flag,
    dump_json,
    dsc_decode,
    main_guard,
    parse_bgi_archive,
    print_rows,
    read_bgi_entry,
    require_out_dir,
    write_entry,
)


def main() -> None:
    ap = argparse.ArgumentParser(description="List or extract BGI PackFile/BURIKO ARC20 archives.")
    ap.add_argument("archive")
    ap.add_argument("--out", help="extract entries to this directory")
    ap.add_argument("--decode-dsc", action="store_true", help="decode DSC FORMAT 1.00 entries while extracting")
    add_json_flag(ap)
    args = ap.parse_args()

    fmt, entries = parse_bgi_archive(args.archive)
    rows = [{"format": fmt, **entry} for entry in entries]
    if args.out:
        out = require_out_dir(args.out)
        written = []
        for entry in entries:
            payload = read_bgi_entry(args.archive, entry)
            if args.decode_dsc and payload.startswith(b"DSC FORMAT 1.00"):
                payload = dsc_decode(payload)
            written.append(str(write_entry(out, entry.name, payload)))
        rows = [{"written": len(written), "files": written}]
    if args.json:
        dump_json(rows)
    else:
        print_rows(rows, ("format", "name", "offset", "size"))


if __name__ == "__main__":
    main_guard(main)
