from __future__ import annotations

import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "check_dynamic_artifacts", ROOT / "Tools" / "check_dynamic_artifacts.py"
)
assert SPEC is not None and SPEC.loader is not None
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


def policy() -> dict:
    return {
        "schema": "astra.dynamic_artifact_policy.v1",
        "artifact": [
            {
                "package": "facade",
                "target": "facade",
                "crate_types": ["rlib", "dylib"],
                "role": "rust_compatibility_facade",
                "windows_export_budget": 60000,
                "allowed_workspace_dependencies": ["core"],
            }
        ],
    }


def metadata(crate_types: list[str], dependencies: list[str] = ["core"]) -> dict:
    packages = [
        {
            "name": "facade",
            "dependencies": [{"name": name, "kind": None} for name in dependencies],
            "targets": [{"name": "facade", "crate_types": crate_types}],
        }
    ]
    packages.extend(
        {"name": name, "dependencies": [], "targets": []} for name in {"core", *dependencies}
    )
    return {"packages": packages}


def minimal_pe(functions: int, names: int) -> bytes:
    payload = bytearray(0x400)
    payload[:2] = b"MZ"
    struct.pack_into("<I", payload, 0x3C, 0x80)
    payload[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", payload, 0x86, 1)
    struct.pack_into("<H", payload, 0x94, 0xF0)
    optional = 0x98
    struct.pack_into("<H", payload, optional, 0x20B)
    struct.pack_into("<II", payload, optional + 112, 0x1000, 40)
    section = optional + 0xF0
    payload[section:section + 8] = b".rdata\0\0"
    struct.pack_into("<IIII", payload, section + 8, 0x200, 0x1000, 0x200, 0x200)
    struct.pack_into(
        "<IIHHIIIIIII", payload, 0x200, 0, 0, 0, 0, 0, 1, functions, names, 0, 0, 0
    )
    return bytes(payload)


class DynamicArtifactChecks(unittest.TestCase):
    def test_rejects_unlisted_dynamic_target(self) -> None:
        metadata_document = metadata(["rlib", "dylib"])
        metadata_document["packages"].append(
            {"name": "unlisted", "dependencies": [], "targets": [{"name": "unlisted", "crate_types": ["cdylib"]}]}
        )
        violations = CHECK.validate_metadata(policy(), metadata_document)
        self.assertIn("DYNAMIC_ARTIFACT_UNAUTHORIZED:unlisted:unlisted", violations)

    def test_rejects_crate_type_and_closure_drift(self) -> None:
        violations = CHECK.validate_metadata(policy(), metadata(["dylib"], ["unexpected"]))
        self.assertIn("DYNAMIC_ARTIFACT_CRATE_TYPES_DRIFT:facade:facade", violations)
        self.assertIn("DYNAMIC_ARTIFACT_CLOSURE_DRIFT:facade", violations)

    def test_counts_named_pe_exports(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.dll"
            path.write_bytes(minimal_pe(60001, 60000))
            self.assertEqual(CHECK.pe_export_counts(path), (60001, 60000))

    def test_rejects_invalid_pe(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "invalid.dll"
            path.write_bytes(b"not-a-pe")
            with self.assertRaises(CHECK.PolicyError):
                CHECK.pe_export_counts(path)


if __name__ == "__main__":
    unittest.main()
