from __future__ import annotations

import hashlib
import io
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import zlib
from collections import deque
from pathlib import Path

from tsuinosora_constants import *

__all__ = ['_normalize_stage3_targets']


def _normalize_stage3_targets(targets: list[dict] | None) -> list[dict]:
    normalized = []
    source = targets or DEFAULT_STAGE3_TARGETS
    for raw in source:
        if not isinstance(raw, dict):
            continue
        target = str(raw.get("target", "")).strip()
        if target not in {"tsuinosora-internal-game", "tsuinosora-patch-game"}:
            continue
        profiles = [
            str(profile).strip()
            for profile in raw.get("profiles", [])
            if str(profile).strip() in {"classic", "modern"}
        ]
        platforms = [
            str(platform).strip()
            for platform in raw.get("platforms", [])
            if str(platform).strip() in {"headless", "windows", "web"}
        ]
        if not profiles:
            profiles = ["classic"]
        if not platforms:
            platforms = ["headless"]
        normalized.append(
            {
                "target": target,
                "profiles": list(dict.fromkeys(profiles)),
                "platforms": list(dict.fromkeys(platforms)),
            }
        )
    if normalized:
        return normalized
    return [dict(spec) for spec in DEFAULT_STAGE3_TARGETS]
