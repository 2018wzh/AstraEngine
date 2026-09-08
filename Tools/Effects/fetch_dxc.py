"""Fetch the pinned DXC runtime for local filter shader compilation.

The archive is never copied into Git. The output directory is explicit so a
caller cannot accidentally overwrite a system DLL or another worktree.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--lock", type=Path, default=Path("Emulator/Assets/Effects/dxc.lock.json"))
    args = parser.parse_args()
    lock = json.loads(args.lock.read_text(encoding="utf-8"))
    if lock.get("version") != "1.8.2502" or len(lock.get("sha256", "")) != 64:
        raise SystemExit("ASTRA_EMU_DXC_LOCK_INVALID")
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="astra_dxc_") as temp:
        archive = Path(temp) / lock["asset"]
        urllib.request.urlretrieve(lock["url"], archive)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if digest != lock["sha256"]:
            raise SystemExit("ASTRA_EMU_DXC_SHA256_MISMATCH")
        with zipfile.ZipFile(archive) as source:
            member = lock["library"]
            if member not in source.namelist():
                raise SystemExit("ASTRA_EMU_DXC_LIBRARY_MISSING")
            destination = args.output / Path(member).name
            with source.open(member) as input_stream, destination.open("wb") as output_stream:
                shutil.copyfileobj(input_stream, output_stream)
    print(args.output / "dxcompiler.dll")
    return 0


if __name__ == "__main__":
    sys.exit(main())
