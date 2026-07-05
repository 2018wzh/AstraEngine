from __future__ import annotations

import argparse

from common import dump_json, file_record, main_guard, printable_strings, read_bytes, u32le


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe BurikoCompiledScriptVer1.00 BCS files.")
    ap.add_argument("file")
    ap.add_argument("--strings", type=int, default=40, help="max string refs to show")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    data = read_bytes(args.file)
    info = file_record(args.file)
    info["is_bcs"] = data.startswith(b"BurikoCompiledScriptVer1.00")
    if len(data) >= 0x20:
        info["header_size_at_0x1c"] = u32le(data, 0x1C)
    info["strings"] = printable_strings(data)[: args.strings]
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
