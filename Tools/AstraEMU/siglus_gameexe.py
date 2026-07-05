from __future__ import annotations

import argparse

from common import dump_json, file_record, main_guard, printable_strings, read_bytes, u32le


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe Siglus Gameexe.dat/Gameexe.chs header and visible strings.")
    ap.add_argument("gameexe")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    data = read_bytes(args.gameexe)
    info = file_record(args.gameexe)
    if len(data) >= 8:
        info["version_or_header0"] = u32le(data, 0)
        info["exe_angou_mode_or_header1"] = u32le(data, 4)
    info["strings"] = printable_strings(data)[:100]
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
