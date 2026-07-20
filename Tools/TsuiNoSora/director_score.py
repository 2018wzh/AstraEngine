"""Strict offline decoder for Director 7 ``VWSC`` score resources.

The decoder intentionally produces ordinary data. It is used only by the
TsuiNoSora conversion tool and is never linked into a shipping player.
"""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256


MAIN_CHANNEL_BYTES = 288
SPRITE_CHANNEL_BYTES = 48
DIRECTOR_7_FRAMES_VERSION = 13
MAX_DIRECTOR_7_CHANNELS = 500


class DirectorScoreError(ValueError):
    """Raised when a score is truncated, inconsistent, or unsupported."""


@dataclass(frozen=True)
class _ScoreLayout:
    detail_count: int
    index_count: int
    detail_offset: int
    offsets: tuple[int, ...]


def decode_director_v7_score(payload: bytes) -> dict:
    """Decode a complete Director 7 score without retaining opaque bytes."""

    layout = _read_layout(payload)
    score_start = layout.detail_offset + layout.offsets[0]
    score_limit = layout.detail_offset + layout.offsets[1]
    if score_limit > len(payload) or score_limit - score_start < 20:
        raise DirectorScoreError("score header entry is truncated")

    score_size = _u32(payload, score_start)
    frame1_offset = _u32(payload, score_start + 4)
    declared_frames = _u32(payload, score_start + 8)
    frames_version = _u16(payload, score_start + 12)
    sprite_record_size = _u16(payload, score_start + 14)
    declared_channels = _u16(payload, score_start + 16)
    reserved_channels = _u16(payload, score_start + 18)

    if frames_version != DIRECTOR_7_FRAMES_VERSION:
        raise DirectorScoreError(f"unsupported Director score frame version: {frames_version}")
    if sprite_record_size != SPRITE_CHANNEL_BYTES:
        raise DirectorScoreError(f"unsupported sprite record size: {sprite_record_size}")
    if frame1_offset != 20:
        raise DirectorScoreError(f"unsupported first frame offset: {frame1_offset}")
    if not 1 <= reserved_channels <= MAX_DIRECTOR_7_CHANNELS:
        raise DirectorScoreError(f"invalid displayed channel count: {reserved_channels}")
    if score_size < frame1_offset or score_start + score_size > score_limit:
        raise DirectorScoreError("score frame stream exceeds detail entry zero")

    channel_state = bytearray(
        MAIN_CHANNEL_BYTES + reserved_channels * SPRITE_CHANNEL_BYTES
    )
    cursor = score_start + frame1_offset
    frame_end = score_start + score_size
    frames: list[dict] = []
    while cursor < frame_end:
        if cursor + 2 > frame_end:
            raise DirectorScoreError("truncated frame size")
        frame_size = _u16(payload, cursor)
        if frame_size == 0:
            raise DirectorScoreError("zero-sized frame is not a valid conversion boundary")
        if frame_size < 2 or cursor + frame_size > frame_end:
            raise DirectorScoreError("frame extends beyond the declared score stream")
        delta_cursor = cursor + 2
        delta_end = cursor + frame_size
        changed_ranges: list[dict] = []
        while delta_cursor < delta_end:
            if delta_cursor + 4 > delta_end:
                raise DirectorScoreError("truncated frame channel delta header")
            size = _u16(payload, delta_cursor)
            offset = _u16(payload, delta_cursor + 2)
            delta_cursor += 4
            if size == 0:
                raise DirectorScoreError("zero-sized channel delta is not allowed")
            if delta_cursor + size > delta_end:
                raise DirectorScoreError("channel delta exceeds its frame")
            if offset + size > len(channel_state):
                raise DirectorScoreError("channel delta exceeds the Director 7 channel buffer")
            channel_state[offset : offset + size] = payload[delta_cursor : delta_cursor + size]
            changed_ranges.append({"offset": offset, "size": size})
            delta_cursor += size
        if delta_cursor != delta_end:
            raise DirectorScoreError("frame channel deltas do not consume the frame")
        frames.append(
            _decode_frame(len(frames) + 1, channel_state, changed_ranges, reserved_channels)
        )
        cursor = delta_end

    if cursor != frame_end:
        raise DirectorScoreError("score frame stream has trailing bytes")
    if not frames:
        raise DirectorScoreError("score contains no frames")

    return {
        "schema": "tsuinosora.director_score_ir.v1",
        "frames_version": frames_version,
        "sprite_record_size": sprite_record_size,
        "displayed_channel_count": reserved_channels,
        "declared_channel_field": declared_channels,
        "reserved_channel_field": reserved_channels,
        "declared_frame_count": declared_frames,
        "decoded_frame_count": len(frames),
        "frame_stream_sha256": f"sha256:{sha256(payload[score_start:frame_end]).hexdigest()}",
        "frames": frames,
    }


def _read_layout(payload: bytes) -> _ScoreLayout:
    if len(payload) < 36:
        raise DirectorScoreError("VWSC resource is too short")
    if _u32(payload, 0) != len(payload):
        raise DirectorScoreError("VWSC declared size does not match its payload")
    if int.from_bytes(payload[4:8], "big", signed=True) != -3:
        raise DirectorScoreError("VWSC is not the supported Director 7 list layout")
    list_start = _u32(payload, 8)
    if list_start < 12 or list_start + 12 > len(payload):
        raise DirectorScoreError("VWSC detail list is out of bounds")
    detail_count = _u32(payload, list_start)
    index_count = _u32(payload, list_start + 4)
    if detail_count < 2 or index_count != detail_count + 1:
        raise DirectorScoreError("VWSC detail index cardinality is inconsistent")
    index_start = list_start + 12
    index_end = index_start + index_count * 4
    if index_end > len(payload):
        raise DirectorScoreError("VWSC detail index is truncated")
    offsets = tuple(_u32(payload, index_start + index * 4) for index in range(index_count))
    detail_bytes = len(payload) - index_end
    if offsets[0] != 0 or offsets[-1] != detail_bytes:
        raise DirectorScoreError("VWSC detail index does not cover the detail section")
    if any(left > right for left, right in zip(offsets, offsets[1:])):
        raise DirectorScoreError("VWSC detail offsets are not monotonic")
    return _ScoreLayout(detail_count, index_count, index_end, offsets)


def _decode_frame(
    number: int,
    state: bytearray,
    changed_ranges: list[dict],
    displayed_channels: int,
) -> dict:
    main = {
        "action": _cast_ref(state, 0),
        "tempo": state[54],
        "tempo_cue_point": _u16(state, 52),
        "transition": _cast_ref(state, 96),
        "sound_2": _cast_ref(state, 144),
        "sound_1": _cast_ref(state, 192),
        "palette": _cast_ref(state, 240, signed=True),
    }
    sprites: list[dict] = []
    for index in range(displayed_channels):
        start = MAIN_CHANNEL_BYTES + index * SPRITE_CHANNEL_BYTES
        member = _u16(state, start + 6)
        cast_library = int.from_bytes(state[start + 4 : start + 6], "big", signed=True)
        y = int.from_bytes(state[start + 12 : start + 14], "big", signed=True)
        x = int.from_bytes(state[start + 14 : start + 16], "big", signed=True)
        height = int.from_bytes(state[start + 16 : start + 18], "big", signed=True)
        width = int.from_bytes(state[start + 18 : start + 20], "big", signed=True)
        if member == 0 and width <= 0 and height <= 0:
            continue
        sprites.append(
            {
                "channel": index + 1,
                "sprite_type": state[start],
                "ink": state[start + 1] & 0x3F,
                "trails": bool(state[start + 1] & 0x40),
                "stretch": bool(state[start + 1] & 0x80),
                "cast_library": cast_library,
                "cast_member": member,
                "sprite_list_index": _u32(state, start + 8),
                "x": x,
                "y": y,
                "width": max(0, width),
                "height": max(0, height),
                "editable": bool(state[start + 20] & 0x40),
                "moveable": bool(state[start + 20] & 0x80),
                "blend": state[start + 21],
            }
        )
    return {"frame": number, "changed_ranges": changed_ranges, "main": main, "sprites": sprites}


def _cast_ref(data: bytes | bytearray, offset: int, *, signed: bool = False) -> dict:
    return {
        "cast_library": int.from_bytes(data[offset : offset + 2], "big", signed=signed),
        "cast_member": int.from_bytes(data[offset + 2 : offset + 4], "big", signed=signed),
    }


def _u16(data: bytes | bytearray, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 2], "big")


def _u32(data: bytes | bytearray, offset: int) -> int:
    return int.from_bytes(data[offset : offset + 4], "big")
