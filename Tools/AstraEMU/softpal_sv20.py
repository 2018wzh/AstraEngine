from __future__ import annotations

import argparse

from common import dump_json, file_record, main_guard, printable_strings, read_bytes, u32le


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe SoftPAL Sv20 SCRIPT.SRC and DAT-like metadata.")
    ap.add_argument("file")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    data = read_bytes(args.file)
    info = file_record(args.file)
    if data.startswith(b"Sv20") and len(data) >= 16:
        info["check"] = f"0x{u32le(data, 4):08x}"
        info["entry"] = f"0x{u32le(data, 8):x}"
        info["declared_size"] = u32le(data, 12)
    info["strings"] = printable_strings(data)[:120]
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
