from __future__ import annotations

import argparse

from common import dump_json, main_guard, parse_fvp_hcb


def main() -> None:
    ap = argparse.ArgumentParser(description="Dump FVP HCB system descriptor and syscall table.")
    ap.add_argument("hcb")
    ap.add_argument("--encoding", default="cp932")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    info = parse_fvp_hcb(args.hcb, args.encoding)
    if args.json:
        dump_json(info)
    else:
        print(f"title={info['title']!r} entry=0x{info['entry_point']:x} syscalls={info['syscall_count']}")
        for row in info["syscalls"]:
            print(f"{row['id']:04d} argc={row['args']} name={row['name']}")


if __name__ == "__main__":
    main_guard(main)
