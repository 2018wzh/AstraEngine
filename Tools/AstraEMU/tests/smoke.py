from __future__ import annotations

import importlib.util
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("common", ROOT / "common.py")
common = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(common)


def write(path: Path, data: bytes) -> Path:
    path.write_bytes(data)
    return path


def make_bgi_pack(path: Path) -> None:
    name = b"hello.txt" + b"\0" * 7
    payload = b"hello"
    header = b"PackFile    " + (1).to_bytes(4, "little")
    entry = name + (0).to_bytes(4, "little") + len(payload).to_bytes(4, "little") + b"\0" * 8
    write(path, header + entry + payload)


def make_fvp_bin(path: Path) -> None:
    name = b"a.txt\0"
    payload = b"abc"
    data = (
        (1).to_bytes(4, "little")
        + len(name).to_bytes(4, "little")
        + (0).to_bytes(4, "little")
        + (0).to_bytes(4, "little")
        + len(payload).to_bytes(4, "little")
        + name
        + payload
    )
    write(path, data)


def make_softpal_pac(path: Path) -> None:
    payload = b"abc"
    header = bytearray(b"PAC " + b"\0" * (0x804 - 4 + 40))
    header[0x0C:0x10] = (0).to_bytes(4, "little")
    header[0x10:0x14] = (1).to_bytes(4, "little")
    rec = 0x804
    header[rec : rec + 5] = b"A.TXT"
    data_off = len(header)
    header[rec + 32 : rec + 36] = len(payload).to_bytes(4, "little")
    header[rec + 36 : rec + 40] = data_off.to_bytes(4, "little")
    write(path, bytes(header) + payload)


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        bgi = base / "a.arc"
        make_bgi_pack(bgi)
        fmt, entries = common.parse_bgi_archive(bgi)
        assert fmt == "PackFile"
        assert entries[0].name == "hello.txt"
        assert common.read_bgi_entry(bgi, entries[0]) == b"hello"

        fvp = base / "a.bin"
        make_fvp_bin(fvp)
        assert common.parse_fvp_bin(fvp)[0]["name"] == "a.txt"

        pac = base / "a.pac"
        make_softpal_pac(pac)
        assert common.parse_softpal_pac(pac)[0]["name"] == "A.TXT"

        try:
            common.safe_join(base, "../bad")
        except common.ToolError:
            pass
        else:
            raise AssertionError("safe_join accepted traversal")

    print("AstraEMU smoke checks passed")


if __name__ == "__main__":
    main()
