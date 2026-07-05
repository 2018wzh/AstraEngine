from __future__ import annotations

import argparse
from collections import Counter

from common import dump_json, file_record, main_guard, printable_strings, read_bytes, u32le


OPCODES = {
    0x00: "nop",
    0x01: "push_byte",
    0x02: "push_word",
    0x03: "push_dword",
    0x04: "push_string",
    0x11: "jmp",
    0x12: "jc",
    0x13: "call",
    0x17: "ret",
}


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe BGI ._bp bytecode headers and opcode histogram.")
    ap.add_argument("file")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    data = read_bytes(args.file)
    header_size = u32le(data, 0) if len(data) >= 4 else 0
    instr_size = u32le(data, 4) if len(data) >= 8 else 0
    code = data[header_size : header_size + instr_size] if header_size < len(data) else b""
    hist = Counter(code)
    info = {
        **file_record(args.file),
        "header_size": header_size,
        "instruction_size": instr_size,
        "opcode_histogram": {OPCODES.get(k, f"0x{k:02x}"): v for k, v in hist.most_common(64)},
        "strings": printable_strings(data[header_size + instr_size :])[:80],
    }
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
