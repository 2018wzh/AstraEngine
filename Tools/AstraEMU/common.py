from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import struct
import sys
from pathlib import Path
from typing import Iterable, Sequence


class ToolError(RuntimeError):
    pass


def read_bytes(path: str | Path, limit: int | None = None) -> bytes:
    with Path(path).open("rb") as f:
        return f.read() if limit is None else f.read(limit)


def u8(data: bytes, off: int) -> int:
    require_range(data, off, 1)
    return data[off]


def u16le(data: bytes, off: int) -> int:
    require_range(data, off, 2)
    return struct.unpack_from("<H", data, off)[0]


def u32le(data: bytes, off: int) -> int:
    require_range(data, off, 4)
    return struct.unpack_from("<I", data, off)[0]


def i32le(data: bytes, off: int) -> int:
    require_range(data, off, 4)
    return struct.unpack_from("<i", data, off)[0]


def u64le(data: bytes, off: int) -> int:
    require_range(data, off, 8)
    return struct.unpack_from("<Q", data, off)[0]


def require_range(data: bytes, off: int, size: int) -> None:
    if off < 0 or size < 0 or off + size > len(data):
        raise ToolError(f"range out of bounds: off=0x{off:x} size=0x{size:x} len=0x{len(data):x}")


def decode_text(data: bytes) -> str:
    for enc in ("utf-8-sig", "utf-16le", "cp932", "gb18030"):
        try:
            text = data.decode(enc)
            if text.count("\ufffd") == 0:
                return text.rstrip("\x00")
        except UnicodeDecodeError:
            pass
    return data.decode("utf-8", errors="replace").rstrip("\x00")


def decode_cstring(data: bytes) -> str:
    return decode_text(data.split(b"\x00", 1)[0])


def printable_strings(data: bytes, min_len: int = 4) -> list[str]:
    out: list[str] = []
    for m in re.finditer(rb"[\x20-\x7e]{%d,}" % min_len, data):
        out.append(m.group(0).decode("ascii", errors="replace"))
    # Simple UTF-16LE scan catches many script labels without parsing the full VM.
    for m in re.finditer((rb"(?:[\x20-\x7e]\x00){%d,}" % min_len), data):
        out.append(m.group(0).decode("utf-16le", errors="replace"))
    return out


def sha256_file(path: str | Path) -> str:
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def file_record(path: str | Path, head: int = 16) -> dict:
    p = Path(path)
    data = read_bytes(p, head)
    return {
        "path": str(p),
        "size": p.stat().st_size,
        "sha256": sha256_file(p),
        "head_hex": data.hex(" "),
        "magic": magic_label(data),
    }


def magic_label(data: bytes) -> str:
    tests: Sequence[tuple[bytes, str]] = (
        (b"PackFile", "BGI PackFile"),
        (b"BURIKO ARC20", "BGI BURIKO ARC20"),
        (b"DSC FORMAT 1.00", "BGI DSC"),
        (b"BurikoCompiledScriptVer1.00", "BGI BCS"),
        (b"CompressedBG___", "BGI CBG"),
        (b"pf6", "Artemis PF6"),
        (b"pf8", "Artemis PF8"),
        (b"PAC ", "SoftPAL PAC"),
        (b"Sv20", "SoftPAL SCRIPT.SRC"),
        (b"pack_scn", "Siglus Scene.pck"),
        (b"TJS2100", "KrKr TJS bytecode"),
        (b"XP3\r\n \n\x1a\x8bg\x01", "KrKr XP3"),
        (b"\x89PNG\r\n\x1a\n", "PNG"),
        (b"OggS", "Ogg"),
        (b"RIFF", "RIFF"),
    )
    for sig, label in tests:
        if data.startswith(sig):
            return label
    return "unknown"


def dump_json(value: object) -> None:
    print(json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True))


def iter_files(root: str | Path, suffixes: Iterable[str] | None = None) -> list[Path]:
    base = Path(root)
    suffix_set = {s.lower() for s in suffixes} if suffixes else None
    files = [p for p in base.rglob("*") if p.is_file()]
    if suffix_set is not None:
        files = [p for p in files if p.suffix.lower() in suffix_set or p.name.lower() in suffix_set]
    return sorted(files)


def require_out_dir(path: str | Path | None) -> Path:
    if not path:
        raise ToolError("extraction/write requires explicit --out")
    out = Path(path).resolve()
    out.mkdir(parents=True, exist_ok=True)
    return out


def safe_join(root: str | Path, entry_name: str) -> Path:
    root_path = Path(root).resolve()
    parts = []
    for raw in entry_name.replace("\\", "/").split("/"):
        if raw in ("", "."):
            continue
        if raw == ".." or any(ch in raw for ch in "\0:*?\"<>|"):
            raise ToolError(f"unsafe archive path: {entry_name!r}")
        parts.append(raw)
    candidate = root_path.joinpath(*parts).resolve()
    if os.path.commonpath([str(root_path), str(candidate)]) != str(root_path):
        raise ToolError(f"path traversal rejected: {entry_name!r}")
    return candidate


def write_entry(out_dir: Path, name: str, payload: bytes) -> Path:
    target = safe_join(out_dir, name)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(payload)
    return target


def main_guard(fn) -> None:
    try:
        fn()
    except ToolError as exc:
        print(f"error: {exc}", file=sys.stderr)
        raise SystemExit(2)


def add_json_flag(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--json", action="store_true", help="emit JSON instead of text")


def print_rows(rows: list[dict], fields: Sequence[str]) -> None:
    for row in rows:
        print(" ".join(f"{field}={row.get(field)!r}" for field in fields))


class BgiEntry(dict):
    @property
    def name(self) -> str:
        return self["name"]

    @property
    def offset(self) -> int:
        return self["offset"]

    @property
    def size(self) -> int:
        return self["size"]


def parse_bgi_archive(path: str | Path) -> tuple[str, list[BgiEntry]]:
    p = Path(path)
    with p.open("rb") as f:
        header = f.read(16)
        if header.startswith(b"PackFile"):
            fmt, name_len, entry_size = "PackFile", 0x10, 0x20
        elif header.startswith(b"BURIKO ARC20"):
            fmt, name_len, entry_size = "BURIKO ARC20", 0x60, 0x80
        else:
            raise ToolError(f"not a BGI archive: {p}")
        count = u32le(header, 12)
        base = 0x10 + count * entry_size
        entries: list[BgiEntry] = []
        for i in range(count):
            raw = f.read(entry_size)
            if len(raw) != entry_size:
                raise ToolError(f"truncated BGI index at entry {i}")
            name = decode_cstring(raw[:name_len])
            rel = u32le(raw, name_len)
            size = u32le(raw, name_len + 4)
            entries.append(BgiEntry(name=name, offset=base + rel, size=size))
    size = p.stat().st_size
    for entry in entries:
        if entry.offset + entry.size > size:
            raise ToolError(f"entry out of bounds: {entry.name}")
    return fmt, entries


def read_bgi_entry(path: str | Path, entry: BgiEntry) -> bytes:
    with Path(path).open("rb") as f:
        f.seek(entry.offset)
        return f.read(entry.size)


def dsc_decode(data: bytes) -> bytes:
    if len(data) < 0x220 or not data.startswith(b"DSC FORMAT 1.00"):
        raise ToolError("missing DSC FORMAT 1.00 header")
    key = u32le(data, 0x10)
    magic = u16le(data, 0) << 16
    out_size = u32le(data, 0x14)
    dec_count = u32le(data, 0x18)
    if out_size > 512 * 1024 * 1024:
        raise ToolError(f"DSC output too large: {out_size}")

    def update_key() -> int:
        nonlocal key
        v0 = 20021 * (key & 0xFFFF)
        v1 = (magic | (key >> 16))
        v1 = (v1 * 20021 + key * 346 + (v0 >> 16)) & 0xFFFF
        key = ((v1 << 16) + (v0 & 0xFFFF) + 1) & 0xFFFFFFFF
        return v1 & 0xFF

    codes = []
    for i in range(512):
        depth = (data[0x20 + i] - update_key()) & 0xFF
        if depth:
            codes.append((depth, i))
    codes.sort()

    nodes = [{"parent": False, "code": None, "left": 0, "right": 0} for _ in range(1024)]
    left = [0] * 512
    right = [0] * 512
    next_node = 1
    depth_nodes = 1
    depth = 0
    left_child = True
    n = 0
    while n < len(codes):
        target_left = left_child
        left_child = not left_child
        existing = 0
        while n < len(codes) and codes[n][0] == depth:
            idx = left[existing] if target_left else right[existing]
            nodes[idx]["code"] = codes[n][1]
            n += 1
            existing += 1
        to_create = max(0, depth_nodes - existing)
        for i in range(to_create):
            idx = left[existing + i] if target_left else right[existing + i]
            nodes[idx]["parent"] = True
            if left_child:
                left[i * 2] = next_node
                nodes[idx]["left"] = next_node
                next_node += 1
                left[i * 2 + 1] = next_node
                nodes[idx]["right"] = next_node
                next_node += 1
            else:
                right[i * 2] = next_node
                nodes[idx]["left"] = next_node
                next_node += 1
                right[i * 2 + 1] = next_node
                nodes[idx]["right"] = next_node
                next_node += 1
        depth += 1
        depth_nodes = to_create * 2

    pos, bits, nbits = 0x220, 0, 0

    def next_bit() -> int:
        nonlocal pos, bits, nbits
        if nbits == 0:
            if pos >= len(data):
                raise ToolError("DSC bitstream exhausted")
            bits = data[pos]
            pos += 1
            nbits = 8
        bit = 1 if bits & 0x80 else 0
        bits = (bits << 1) & 0xFF
        nbits -= 1
        return bit

    def next_bits(count: int) -> int:
        value = 0
        for _ in range(count):
            value = (value << 1) | next_bit()
        return value

    out = bytearray(out_size)
    dst = 0
    for _ in range(dec_count):
        node_idx = 0
        while True:
            node_idx = nodes[node_idx]["right" if next_bit() else "left"]
            node = nodes[node_idx]
            if not node["parent"]:
                code = node["code"]
                if code is None:
                    raise ToolError("DSC missing leaf code")
                if code >> 8 == 1:
                    offset = next_bits(12) + 2
                    count = (code & 0xFF) + 2
                    if offset > dst:
                        raise ToolError("DSC back-reference before output start")
                    for _ in range(count):
                        if dst >= out_size:
                            break
                        out[dst] = out[dst - offset]
                        dst += 1
                elif dst < out_size:
                    out[dst] = code & 0xFF
                    dst += 1
                break
        if dst >= out_size:
            break
    return bytes(out)


def parse_fvp_bin(path: str | Path) -> list[dict]:
    data = read_bytes(path)
    count = u32le(data, 0)
    table_size = u32le(data, 4)
    if count > 1_000_000:
        raise ToolError(f"unreasonable FVP bin entry count: {count}")
    records_off = 8
    names_off = records_off + count * 12
    data_base = names_off + table_size
    entries = []
    for i in range(count):
        off = records_off + i * 12
        name_off = u32le(data, off)
        data_off = u32le(data, off + 4)
        size = u32le(data, off + 8)
        name_start = names_off + name_off
        if name_start >= len(data):
            name = f"<bad-name-{i}>"
        else:
            name = decode_cstring(data[name_start:])
        entries.append({"name": name, "offset": data_base + data_off, "size": size})
    return entries


def parse_fvp_hcb(path: str | Path, encoding: str = "cp932") -> dict:
    data = read_bytes(path)
    sysdesc = u32le(data, 0)
    off = sysdesc
    entry = u32le(data, off)
    nv = u16le(data, off + 4)
    vv = u16le(data, off + 6)
    mode = u8(data, off + 8)
    title_len = u8(data, off + 10)
    title = data[off + 11 : off + 11 + title_len].decode(encoding, errors="replace").rstrip("\x00")
    p = off + 11 + title_len
    syscall_count = u16le(data, p)
    p += 2
    syscalls = []
    for i in range(syscall_count):
        argc = u8(data, p)
        name_len = u8(data, p + 1)
        p += 2
        name = data[p : p + name_len].decode(encoding, errors="replace").rstrip("\x00")
        p += name_len
        syscalls.append({"id": i, "args": argc, "name": name})
    return {
        "sys_desc_offset": sysdesc,
        "entry_point": entry,
        "non_volatile_globals": nv,
        "volatile_globals": vv,
        "game_mode": mode,
        "title": title,
        "syscall_count": syscall_count,
        "syscalls": syscalls,
    }


def parse_pf_archive(path: str | Path) -> tuple[str, list[dict], bytes | None]:
    data = read_bytes(path)
    if not (data.startswith(b"pf6") or data.startswith(b"pf8")):
        raise ToolError(f"not a PF6/PF8 archive: {path}")
    fmt = data[:3].decode("ascii")
    index_size = u32le(data, 3)
    count = u32le(data, 7)
    cursor = 0x0B
    index_end = min(len(data), 0x07 + index_size)
    entries = []
    for _ in range(count):
        name_len = u32le(data, cursor)
        cursor += 4
        name = decode_text(data[cursor : cursor + name_len]).rstrip("\x00")
        cursor += name_len + 4
        offset = u32le(data, cursor)
        size = u32le(data, cursor + 4)
        cursor += 8
        entries.append({"name": name.replace("\\", "/"), "offset": offset, "size": size, "encrypted": fmt == "pf8" and not (name.endswith("mp4") or name.endswith("flv"))})
        if cursor > index_end:
            break
    key = hashlib.sha1(data[0x07 : 0x07 + index_size]).digest() if fmt == "pf8" else None
    return fmt, entries, key


def read_pf_entry(path: str | Path, entry: dict, key: bytes | None) -> bytes:
    with Path(path).open("rb") as f:
        f.seek(entry["offset"])
        data = bytearray(f.read(entry["size"]))
    if entry.get("encrypted"):
        if not key:
            raise ToolError("encrypted PF8 entry has no key")
        for i in range(len(data)):
            data[i] ^= key[i % len(key)]
    return bytes(data)


def parse_softpal_pac(path: str | Path) -> list[dict]:
    data = read_bytes(path)
    if len(data) < 0x804 or not data.startswith(b"PAC "):
        raise ToolError(f"not a SoftPAL PAC archive: {path}")
    entries = []
    for bucket in range(255):
        off = 0x0C + bucket * 8
        first = u32le(data, off)
        count = u32le(data, off + 4)
        for i in range(count):
            rec = 0x804 + (first + i) * 40
            key = data[rec : rec + 32].rstrip(b"\x00")
            size = u32le(data, rec + 32)
            data_off = u32le(data, rec + 36)
            entries.append({
                "bucket": bucket,
                "name": decode_text(key),
                "key_hex": key.hex(),
                "offset": data_off,
                "size": size,
            })
    return entries


def parse_siglus_scene_header(path: str | Path) -> dict:
    data = read_bytes(path, 128)
    has_sig = data.startswith(b"pack_scn")
    p = 8 if has_sig else 0
    fields = [i32le(data, p + i * 4) for i in range(23)]
    names = [
        "header_size", "inc_prop_list_ofs", "inc_prop_cnt", "inc_prop_name_index_list_ofs",
        "inc_prop_name_index_cnt", "inc_prop_name_list_ofs", "inc_prop_name_cnt",
        "inc_cmd_list_ofs", "inc_cmd_cnt", "inc_cmd_name_index_list_ofs",
        "inc_cmd_name_index_cnt", "inc_cmd_name_list_ofs", "inc_cmd_name_cnt",
        "scn_name_index_list_ofs", "scn_name_index_cnt", "scn_name_list_ofs",
        "scn_name_cnt", "scn_data_index_list_ofs", "scn_data_index_cnt",
        "scn_data_list_ofs", "scn_data_cnt", "scn_data_exe_angou_mod",
        "original_source_header_size",
    ]
    out = dict(zip(names, fields))
    out["has_signature"] = has_sig
    out["path"] = str(path)
    out["size"] = Path(path).stat().st_size
    return out


def parse_xp3(path: str | Path) -> list[dict]:
    data = read_bytes(path)
    sig = b"XP3\r\n \n\x1a\x8bg\x01"
    if not data.startswith(sig):
        raise ToolError(f"not an XP3 archive: {path}")
    index_offset = u64le(data, len(sig))
    require_range(data, index_offset, 1)
    flag = data[index_offset]
    p = index_offset + 1
    if flag == 1:
        comp_size = u64le(data, p)
        raw_size = u64le(data, p + 8)
        import zlib
        index = zlib.decompress(data[p + 16 : p + 16 + comp_size])
        if len(index) != raw_size:
            raise ToolError("XP3 index size mismatch")
    elif flag == 0:
        raw_size = u64le(data, p)
        index = data[p + 8 : p + 8 + raw_size]
    else:
        raise ToolError(f"unsupported XP3 index flag: {flag}")
    entries = []
    p = 0
    while p + 12 <= len(index):
        tag = index[p : p + 4]
        size = u64le(index, p + 4)
        chunk = index[p + 12 : p + 12 + size]
        p += 12 + size
        if tag != b"File":
            continue
        entries.append(parse_xp3_file_chunk(chunk))
    return entries


def parse_xp3_file_chunk(chunk: bytes) -> dict:
    p = 0
    row: dict = {"name": "<unknown>", "segments": []}
    while p + 12 <= len(chunk):
        tag = chunk[p : p + 4]
        size = u64le(chunk, p + 4)
        body = chunk[p + 12 : p + 12 + size]
        p += 12 + size
        if tag == b"info":
            row["flags"] = u32le(body, 0)
            row["original_size"] = u64le(body, 4)
            row["archive_size"] = u64le(body, 12)
            name_len = u16le(body, 20)
            raw = body[22 : 22 + name_len * 2]
            row["name"] = raw.decode("utf-16le", errors="replace")
        elif tag == b"segm":
            q = 0
            while q + 28 <= len(body):
                row["segments"].append({
                    "flags": u32le(body, q),
                    "offset": u64le(body, q + 4),
                    "compressed_size": u64le(body, q + 12),
                    "original_size": u64le(body, q + 20),
                })
                q += 28
        elif tag == b"adlr":
            row["adler32"] = u32le(body, 0)
    row["size"] = row.get("archive_size", sum(s["compressed_size"] for s in row["segments"]))
    return row
