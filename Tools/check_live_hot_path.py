#!/usr/bin/env python3
"""Reject serialization and content hashing from dedicated live-path modules."""

from __future__ import annotations

import json
from pathlib import Path


FILES = (
    "Engine/Source/Runtime/astra-plugin-abi/src/live_presentation.rs",
    "Engine/Source/Runtime/astra-plugin-abi/src/live_vn_state.rs",
    "Engine/Source/Runtime/astra-audio-kira/src/backend.rs",
    "Engine/Source/Runtime/astra-audio-kira/src/sound.rs",
    "Engine/Source/Runtime/astra-audio-kira/src/stream.rs",
    "Engine/Source/Runtime/astra-audio-kira/src/timeline.rs",
    "Engine/Source/Runtime/astra-media/src/text_layout/provider.rs",
    "Engine/Source/Runtime/astra-media/src/text_layout/layout_engine.rs",
    "Engine/Source/Modules/AstraVN/astra-vn-ui/src/model.rs",
    "Engine/Source/Modules/AstraVN/astra-vn-ui/src/controller.rs",
)

FORBIDDEN = (
    "postcard::",
    "serde_json::",
    "content_hash",
    "payload_hash",
    "Hash128::from_",
    "Hash256::from_",
)


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    violations: list[dict[str, object]] = []
    for relative in FILES:
        path = root / relative
        text = path.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), start=1):
            for token in FORBIDDEN:
                if token in line:
                    violations.append(
                        {"file": relative, "line": line_number, "token": token}
                    )
    report = {
        "schema": "astra.live_hot_path_guard.v1",
        "status": "blocked" if violations else "pass",
        "files": list(FILES),
        "violations": violations,
    }
    print(json.dumps(report, sort_keys=True))
    return 1 if violations else 0


if __name__ == "__main__":
    raise SystemExit(main())
