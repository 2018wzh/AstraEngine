from __future__ import annotations

import argparse

from common import dump_json, main_guard, parse_siglus_scene_header


def main() -> None:
    ap = argparse.ArgumentParser(description="Inspect Siglus Scene.pck pack_scn header.")
    ap.add_argument("scene_pck")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    info = parse_siglus_scene_header(args.scene_pck)
    dump_json(info) if args.json else print(info)


if __name__ == "__main__":
    main_guard(main)
