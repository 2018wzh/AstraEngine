from __future__ import annotations

import argparse
from pathlib import Path

from common import dump_json, file_record, main_guard, printable_strings, read_bytes, ToolError


def load_key_file(path: str | None) -> bytes | None:
    if not path:
        return None
    text = Path(path).read_text(encoding="utf-8").strip()
    compact = "".join(ch for ch in text if ch in "0123456789abcdefABCDEF")
    if len(compact) % 2:
        raise ToolError("key file must contain even-length hex")
    return bytes.fromhex(compact)


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe Minori PAZ archives. Keys are external via --key-file; no built-in keys.")
    ap.add_argument("paz")
    ap.add_argument("--key-file", help="hex key material owned by the operator")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    data = read_bytes(args.paz, 4096)
    key = load_key_file(args.key_file)
    info = file_record(args.paz)
    info["key_supplied"] = key is not None
    info["note"] = "TOC decode is intentionally key-driven; this tool does not embed PAZ keys."
    info["visible_strings"] = printable_strings(data)[:40]
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
