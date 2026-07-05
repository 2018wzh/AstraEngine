from __future__ import annotations

import argparse
from pathlib import Path

from common import dsc_decode, dump_json, file_record, main_guard


def main() -> None:
    ap = argparse.ArgumentParser(description="Inspect or decode one BGI DSC FORMAT 1.00 payload.")
    ap.add_argument("file")
    ap.add_argument("--out", help="write decoded payload to this file")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    raw = Path(args.file).read_bytes()
    decoded = dsc_decode(raw)
    if args.out:
        Path(args.out).write_bytes(decoded)
    info = {**file_record(args.file), "decoded_size": len(decoded), "wrote": args.out}
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
