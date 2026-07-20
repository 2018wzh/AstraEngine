"""Strict decoder for the documented ProjectorRays JSON dialect.

ProjectorRays emits JSON through its own encoder.  That encoder follows JSON
for structure and Unicode escaping, but additionally writes ``\\v`` and
``\\xHH`` escapes.  This module normalizes only those two proven extensions
before delegating all syntax and structure validation to Python's JSON parser.
"""

from __future__ import annotations

import json
import string


_HEX_DIGITS = frozenset(string.hexdigits)


def loads_projectorrays_json(text: str):
    """Decode ProjectorRays JSON without accepting unrelated broken syntax."""

    return json.loads(_normalize_projectorrays_escapes(text))


def decode_projectorrays_byte_text(value: str, encoding: str = "cp932") -> str:
    """Decode a ProjectorRays byte-escaped field with an explicit source codec.

    ProjectorRays represents non-ASCII bytes as ``\\xHH`` in some metadata
    fields. ``loads_projectorrays_json`` deliberately preserves those bytes as
    U+00HH so generic JSON parsing remains lossless. Callers must opt in for
    fields whose source contract identifies a concrete text encoding.
    """

    if not isinstance(value, str):
        raise TypeError("ProjectorRays byte text must be a string")
    if any(ord(character) > 0xFF for character in value):
        return value
    try:
        return bytes(ord(character) for character in value).decode(encoding)
    except (LookupError, UnicodeDecodeError) as exc:
        raise ValueError(f"ProjectorRays byte text is not valid {encoding}") from exc


def _normalize_projectorrays_escapes(text: str) -> str:
    output: list[str] = []
    in_string = False
    index = 0
    while index < len(text):
        character = text[index]
        if character == '"':
            in_string = not in_string
            output.append(character)
            index += 1
            continue
        if not in_string or character != "\\":
            output.append(character)
            index += 1
            continue

        if index + 1 >= len(text):
            output.append(character)
            index += 1
            continue

        escape = text[index + 1]
        if escape == "v":
            output.append("\\u000b")
            index += 2
            continue
        if escape == "x":
            end = index + 4
            digits = text[index + 2 : end]
            if len(digits) != 2 or any(digit not in _HEX_DIGITS for digit in digits):
                raise json.JSONDecodeError("invalid ProjectorRays hexadecimal escape", text, index)
            output.append(f"\\u00{digits}")
            index = end
            continue

        # Preserve standard escapes exactly.  Appending the escaped character
        # here also prevents an escaped quote from changing string state.
        output.append(character)
        output.append(escape)
        index += 2

    return "".join(output)
