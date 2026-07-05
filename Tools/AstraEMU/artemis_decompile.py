from __future__ import annotations

import argparse
import ast
import re

from common import decode_text, dump_json, main_guard, printable_strings, read_bytes


TAG_RE = re.compile(r"\[([A-Za-z0-9_:.]+)([^\]]*)\]")
LABEL_RE = re.compile(r"^\*([^\s]+)", re.M)


def decompile_text_script(data: bytes) -> dict:
    text = decode_text(data)
    return {
        "labels": LABEL_RE.findall(text),
        "tags": [{"name": m.group(1), "raw": m.group(0)} for m in TAG_RE.finditer(text)],
        "text_lines": [line for line in text.splitlines() if line and not line.startswith(("*", ";", "["))][:200],
    }


def decompile_ast(data: bytes) -> dict:
    text = decode_text(data)
    tags = [{"name": m.group(1), "raw": m.group(0)} for m in TAG_RE.finditer(text)]
    return {"kind": "ast-lua-table", "tags": tags, "strings": printable_strings(data)[:200]}


def decompile_asb(data: bytes) -> dict:
    return {"kind": "asb", "has_magic": data.startswith(b"ASB\x00"), "strings": printable_strings(data)[:200]}


def main() -> None:
    ap = argparse.ArgumentParser(description="Lightweight Artemis IET/AST/ASB/SLI/IPT/TBL string and tag dumper.")
    ap.add_argument("file")
    ap.add_argument("--kind", choices=["auto", "iet", "ast", "asb", "sli", "ipt", "tbl"], default="auto")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    data = read_bytes(args.file)
    kind = args.kind
    if kind == "auto":
        lower = args.file.lower()
        kind = "asb" if data.startswith(b"ASB\x00") else "ast" if lower.endswith(".ast") else "iet"
    result = decompile_asb(data) if kind == "asb" else decompile_ast(data) if kind == "ast" else decompile_text_script(data)
    result["kind"] = kind
    dump_json(result) if args.json else print(result)


if __name__ == "__main__":
    main_guard(main)
