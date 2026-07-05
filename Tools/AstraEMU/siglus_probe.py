from __future__ import annotations

import argparse
from collections import Counter

from common import dump_json, iter_files, magic_label, main_guard, parse_siglus_scene_header, read_bytes


def main() -> None:
    ap = argparse.ArgumentParser(description="Probe a Siglus game root.")
    ap.add_argument("root")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    ext = Counter()
    scenes = []
    gameexe = []
    for path in iter_files(args.root):
        ext[path.suffix.lower() or "<none>"] += 1
        if path.name.lower() == "scene.pck":
            scenes.append(parse_siglus_scene_header(path))
        elif path.name.lower().startswith("gameexe."):
            gameexe.append({"path": str(path), "magic": magic_label(read_bytes(path, 16)), "size": path.stat().st_size})
    report = {"root": args.root, "extension_counts": dict(ext), "scenes": scenes, "gameexe": gameexe}
    dump_json(report) if args.json else print(report)


if __name__ == "__main__":
    main_guard(main)
