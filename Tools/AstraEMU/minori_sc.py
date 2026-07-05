from __future__ import annotations

import argparse
import re

from common import decode_text, dump_json, main_guard, read_bytes


MESSAGE_RE = re.compile(r"(message|select|voice|bgm|se|image)", re.I)


def main() -> None:
    ap = argparse.ArgumentParser(description="Extract lightweight strings and command markers from decoded Minori .sc-like files.")
    ap.add_argument("file")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    text = decode_text(read_bytes(args.file))
    lines = [line for line in text.splitlines() if line.strip()]
    info = {
        "markers": [line for line in lines if MESSAGE_RE.search(line)][:200],
        "text_lines": [line for line in lines if not line.startswith(("#", "//", ";"))][:200],
    }
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
