#!/usr/bin/env python3
"""Validate the workspace dynamic-artifact policy and Windows dylib budgets."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tomllib
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
POLICY_PATH = ROOT / "Tools" / "dynamic_artifact_policy.toml"
DYNAMIC_CRATE_TYPES = {"cdylib", "dylib"}


class PolicyError(ValueError):
    """A dynamic-artifact policy or build result is invalid."""


def cargo_metadata(root: Path) -> dict[str, Any]:
    completed = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return json.loads(completed.stdout.decode("utf-8"))


def load_policy(path: Path) -> dict[str, Any]:
    with path.open("rb") as source:
        policy = tomllib.load(source)
    if policy.get("schema") != "astra.dynamic_artifact_policy.v1":
        raise PolicyError("DYNAMIC_ARTIFACT_POLICY_SCHEMA_INVALID")
    return policy


def policy_artifacts(policy: dict[str, Any]) -> dict[tuple[str, str], dict[str, Any]]:
    result: dict[tuple[str, str], dict[str, Any]] = {}
    for artifact in policy.get("artifact", []):
        key = (artifact.get("package", ""), artifact.get("target", ""))
        if not all(key) or key in result:
            raise PolicyError("DYNAMIC_ARTIFACT_POLICY_DUPLICATE_OR_MISSING_ARTIFACT")
        crate_types = set(artifact.get("crate_types", []))
        if not crate_types & DYNAMIC_CRATE_TYPES:
            raise PolicyError("DYNAMIC_ARTIFACT_POLICY_MISSING_DYNAMIC_CRATE_TYPE")
        if artifact.get("role") not in {
            "rust_compatibility_facade",
            "platform_host",
            "test_fixture_provider",
            "family_plugin",
            "emulator_runtime",
            "program_host",
        }:
            raise PolicyError("DYNAMIC_ARTIFACT_POLICY_ROLE_INVALID")
        if artifact.get("role") == "rust_compatibility_facade":
            budget = artifact.get("windows_export_budget")
            if not isinstance(budget, int) or not 0 < budget <= 65535:
                raise PolicyError("DYNAMIC_ARTIFACT_POLICY_EXPORT_BUDGET_INVALID")
        result[key] = artifact
    return result


def is_local_package(package: dict[str, Any], root: Path) -> bool:
    manifest_path = package.get("manifest_path")
    return manifest_path is None or Path(manifest_path).resolve().is_relative_to(root)


def dynamic_targets(metadata: dict[str, Any], root: Path = ROOT) -> dict[tuple[str, str], set[str]]:
    targets: dict[tuple[str, str], set[str]] = {}
    for package in metadata["packages"]:
        if not is_local_package(package, root):
            continue
        for target in package["targets"]:
            crate_types = set(target["crate_types"])
            if crate_types & DYNAMIC_CRATE_TYPES:
                key = (package["name"], target["name"])
                if key in targets:
                    raise PolicyError("DYNAMIC_ARTIFACT_WORKSPACE_DUPLICATE_TARGET")
                targets[key] = crate_types
    return targets


def direct_workspace_dependencies(metadata: dict[str, Any], root: Path = ROOT) -> dict[str, set[str]]:
    workspace_packages = {
        package["name"] for package in metadata["packages"] if is_local_package(package, root)
    }
    result: dict[str, set[str]] = {}
    for package in metadata["packages"]:
        if package["name"] not in workspace_packages:
            continue
        result[package["name"]] = {
            dependency["name"]
            for dependency in package["dependencies"]
            if dependency["name"] in workspace_packages and dependency.get("kind") is None
        }
    return result


def validate_metadata(policy: dict[str, Any], metadata: dict[str, Any]) -> list[str]:
    violations: list[str] = []
    expected = policy_artifacts(policy)
    actual = dynamic_targets(metadata)
    for key, crate_types in sorted(actual.items()):
        artifact = expected.get(key)
        if artifact is None:
            violations.append(f"DYNAMIC_ARTIFACT_UNAUTHORIZED:{key[0]}:{key[1]}")
            continue
        if crate_types != set(artifact["crate_types"]):
            violations.append(f"DYNAMIC_ARTIFACT_CRATE_TYPES_DRIFT:{key[0]}:{key[1]}")
    for key in sorted(expected):
        if key not in actual:
            violations.append(f"DYNAMIC_ARTIFACT_DECLARED_TARGET_MISSING:{key[0]}:{key[1]}")

    dependencies = direct_workspace_dependencies(metadata)
    for (package, _target), artifact in expected.items():
        allowed = artifact.get("allowed_workspace_dependencies")
        if allowed is None:
            continue
        actual_dependencies = dependencies.get(package, set())
        if actual_dependencies != set(allowed):
            violations.append(f"DYNAMIC_ARTIFACT_CLOSURE_DRIFT:{package}")
    return violations


def validate_documentation(policy: dict[str, Any], root: Path) -> list[str]:
    documentation = policy.get("documentation", {})
    command = documentation.get("required_command", "")
    violations: list[str] = []
    for relative_path in documentation.get("paths", []):
        path = root / relative_path
        if not path.is_file() or command not in path.read_text(encoding="utf-8"):
            violations.append(f"DYNAMIC_ARTIFACT_DOCUMENTATION_DRIFT:{relative_path}")
    return violations


def rva_to_offset(payload: bytes, rva: int, section_offset: int, section_count: int) -> int:
    for index in range(section_count):
        offset = section_offset + index * 40
        virtual_size, virtual_address, raw_size, raw_offset = struct.unpack_from("<IIII", payload, offset + 8)
        section_size = max(virtual_size, raw_size)
        if virtual_address <= rva < virtual_address + section_size:
            return raw_offset + rva - virtual_address
    raise PolicyError("DYNAMIC_ARTIFACT_PE_RVA_OUT_OF_RANGE")


def pe_export_counts(path: Path) -> tuple[int, int]:
    payload = path.read_bytes()
    if len(payload) < 64 or payload[:2] != b"MZ":
        raise PolicyError("DYNAMIC_ARTIFACT_PE_DOS_HEADER_INVALID")
    pe_offset = struct.unpack_from("<I", payload, 0x3C)[0]
    if pe_offset + 24 > len(payload) or payload[pe_offset : pe_offset + 4] != b"PE\0\0":
        raise PolicyError("DYNAMIC_ARTIFACT_PE_HEADER_INVALID")
    section_count = struct.unpack_from("<H", payload, pe_offset + 6)[0]
    optional_size = struct.unpack_from("<H", payload, pe_offset + 20)[0]
    optional_offset = pe_offset + 24
    if optional_offset + optional_size > len(payload):
        raise PolicyError("DYNAMIC_ARTIFACT_PE_OPTIONAL_HEADER_INVALID")
    magic = struct.unpack_from("<H", payload, optional_offset)[0]
    directory_offset = optional_offset + (112 if magic == 0x20B else 96 if magic == 0x10B else -1)
    if directory_offset < optional_offset or directory_offset + 8 > optional_offset + optional_size:
        raise PolicyError("DYNAMIC_ARTIFACT_PE_OPTIONAL_MAGIC_INVALID")
    export_rva, export_size = struct.unpack_from("<II", payload, directory_offset)
    if export_rva == 0 or export_size == 0:
        return (0, 0)
    section_offset = optional_offset + optional_size
    export_offset = rva_to_offset(payload, export_rva, section_offset, section_count)
    if export_offset + 40 > len(payload):
        raise PolicyError("DYNAMIC_ARTIFACT_PE_EXPORT_DIRECTORY_INVALID")
    fields = struct.unpack_from("<IIHHIIIIIII", payload, export_offset)
    return (fields[6], fields[7])


def build_windows_dylib(package: str, target: str, root: Path) -> Path:
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(root / "target" / "dynamic-artifact-audit")
    completed = subprocess.run(
        ["cargo", "build", "--locked", "-p", package, "--lib", "--message-format", "json-render-diagnostics"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
    )
    for line in completed.stdout.decode("utf-8").splitlines():
        message = json.loads(line)
        if message.get("reason") != "compiler-artifact" or message.get("target", {}).get("name") != target:
            continue
        for filename in message.get("filenames", []):
            candidate = Path(filename)
            if candidate.suffix.lower() == ".dll":
                return candidate
    raise PolicyError("DYNAMIC_ARTIFACT_WINDOWS_DLL_MISSING")


def verify_windows_exports(policy: dict[str, Any], root: Path) -> tuple[list[str], list[dict[str, Any]]]:
    violations: list[str] = []
    audits: list[dict[str, Any]] = []
    for artifact in policy_artifacts(policy).values():
        if not artifact.get("audit_windows_exports"):
            continue
        package = artifact["package"]
        target = artifact["target"]
        try:
            functions, names = pe_export_counts(build_windows_dylib(package, target, root))
            budget = artifact["windows_export_budget"]
            audits.append({"package": package, "functions": functions, "named_exports": names, "budget": budget})
            if names > budget:
                violations.append(f"DYNAMIC_ARTIFACT_EXPORT_BUDGET_EXCEEDED:{package}:{names}:{budget}")
        except (OSError, subprocess.CalledProcessError, json.JSONDecodeError, PolicyError) as error:
            violations.append(f"DYNAMIC_ARTIFACT_EXPORT_AUDIT_FAILED:{package}:{error}")
    return violations, audits


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--verify-windows-exports", action="store_true")
    arguments = parser.parse_args()
    try:
        policy = load_policy(POLICY_PATH)
        violations = validate_metadata(policy, cargo_metadata(ROOT))
        violations.extend(validate_documentation(policy, ROOT))
        audits: list[dict[str, Any]] = []
        if arguments.verify_windows_exports:
            export_violations, audits = verify_windows_exports(policy, ROOT)
            violations.extend(export_violations)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError, tomllib.TOMLDecodeError, PolicyError) as error:
        print(json.dumps({"schema": "astra.dynamic_artifact_report.v1", "status": "blocked", "diagnostic": str(error)}, sort_keys=True))
        return 1
    report = {
        "schema": "astra.dynamic_artifact_report.v1",
        "status": "pass" if not violations else "blocked",
        "violations": violations,
        "windows_export_audits": audits,
    }
    print(json.dumps(report, sort_keys=True))
    return 0 if not violations else 1


if __name__ == "__main__":
    raise SystemExit(main())
