from __future__ import annotations

import argparse
import re

from common import decode_text, dump_json, main_guard, read_bytes


TAG_RE = re.compile(r"\[([A-Za-z0-9_]+)([^\]]*)\]")
LABEL_RE = re.compile(r"^\*([^\s|]+)", re.M)


def main() -> None:
    ap = argparse.ArgumentParser(description="Extract lightweight KAG .ks labels, tags, and dialogue-like lines.")
    ap.add_argument("file")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    text = decode_text(read_bytes(args.file))
    info = {
        "labels": LABEL_RE.findall(text),
        "tags": [{"name": m.group(1), "raw": m.group(0)} for m in TAG_RE.finditer(text)],
        "text_lines": [line for line in text.splitlines() if line and not line.startswith(("*", ";", "@", "["))][:200],
    }
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
