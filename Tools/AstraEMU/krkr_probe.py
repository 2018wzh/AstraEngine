from __future__ import annotations

import argparse

from common import dump_json, iter_files, main_guard, parse_xp3


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe a KrKr game root and XP3 patch stack.")
    ap.add_argument("root")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    packs = []
    for path in iter_files(args.root, [".xp3"]):
        try:
            entries = parse_xp3(path)
            packs.append({"path": str(path), "entries": len(entries), "sample": [e["name"] for e in entries[:10]]})
        except Exception as exc:
            packs.append({"path": str(path), "error": str(exc)})
    report = {"root": args.root, "xp3": packs}
    dump_json(report) if args.json else print(report)


if __name__ == "__main__":
    main_guard(main)
