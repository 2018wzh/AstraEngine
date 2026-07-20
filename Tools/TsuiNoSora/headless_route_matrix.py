#!/usr/bin/env python3
"""Run every generated TsuiNoSora route through the real Headless product host."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path


SCHEMA = "tsuinosora.headless_route_matrix_report.v1"
INPUT_SCHEMA = "astra.user_input_sequence.v1"
RUN_REPORT_SCHEMA = "astra.headless_run_report.v1"


class RouteMatrixError(RuntimeError):
    pass


@dataclass(frozen=True)
class RouteContract:
    route_id: str
    terminal_id: str
    terminal_route_node_id: str
    choice_ids: tuple[str, ...]
    choice_sequence: tuple[str, ...]
    input_path: Path
    message_count: int
    input_sequence_hash: str


def _json_hash(value: object) -> str:
    encoded = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode(
        "utf-8"
    )
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


def _file_hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def _input_sequence_hash(rows: list[dict]) -> str:
    digest = hashlib.sha256()
    for row in rows:
        digest.update(
            json.dumps(row, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        )
        digest.update(b"\n")
    return f"sha256:{digest.hexdigest()}"


def _load_json(path: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RouteMatrixError(f"JSON input is unreadable: {path.name}: {error}") from error
    if not isinstance(value, dict):
        raise RouteMatrixError(f"JSON input must be an object: {path.name}")
    return value


def _load_jsonl(path: Path) -> list[dict]:
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise RouteMatrixError(f"input sequence is unreadable: {path.name}: {error}") from error
    rows = []
    for line_number, line in enumerate(lines, start=1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise RouteMatrixError(
                f"input sequence has invalid JSON at {path.name}:{line_number}: {error.msg}"
            ) from error
        if not isinstance(row, dict):
            raise RouteMatrixError(f"input sequence row is not an object at {path.name}:{line_number}")
        rows.append(row)
    if not rows:
        raise RouteMatrixError(f"input sequence is empty: {path.name}")
    return rows


def _validate_route_input(path: Path, route: dict) -> RouteContract:
    route_id = route.get("route_id")
    terminal_id = route.get("terminal_id")
    terminal_route_node_id = route.get("terminal_route_node_id")
    choice_ids = route.get("choice_ids")
    choice_sequence = route.get("choice_sequence")
    if not isinstance(route_id, str) or not route_id.startswith("route.coverage."):
        raise RouteMatrixError("route IR has an invalid route_id")
    if not isinstance(terminal_id, str) or not terminal_id:
        raise RouteMatrixError(f"{route_id} has an invalid terminal_id")
    if terminal_route_node_id != f"state.{terminal_id}":
        raise RouteMatrixError(f"{route_id} has an invalid terminal_route_node_id")
    if not isinstance(choice_ids, list) or not all(isinstance(item, str) for item in choice_ids):
        raise RouteMatrixError(f"{route_id} has an invalid choice signature")
    if len(choice_ids) != len(set(choice_ids)):
        raise RouteMatrixError(f"{route_id} has duplicate choice ids")
    if (
        not isinstance(choice_sequence, list)
        or not all(isinstance(item, str) for item in choice_sequence)
        or list(dict.fromkeys(choice_sequence)) != choice_ids
    ):
        raise RouteMatrixError(f"{route_id} has an invalid ordered choice sequence")

    rows = _load_jsonl(path)
    expected_session = f"tsui.{route_id}"
    for expected_sequence, row in enumerate(rows, start=1):
        if row.get("schema") != INPUT_SCHEMA:
            raise RouteMatrixError(f"{route_id} input row has an invalid schema")
        if row.get("session") != expected_session:
            raise RouteMatrixError(f"{route_id} input rows do not share the expected session")
        if row.get("sequence") != expected_sequence:
            raise RouteMatrixError(f"{route_id} input sequence is not contiguous")

    events = [row.get("event") for row in rows]
    expected_terminal_hash = _json_hash([terminal_route_node_id])
    terminal_evidence = any(
        isinstance(event, dict)
        and event.get("type") == "await"
        and event.get("observation")
        == {
            "kind": "equals",
            "key": "vn.terminal_routes",
            "value_hash": expected_terminal_hash,
        }
        and event.get("continue_at_match") is True
        for event in events
    )
    if not terminal_evidence:
        raise RouteMatrixError(f"{route_id} does not assert its terminal route observation")
    if events[-2] != {"type": "checkpoint", "id": f"checkpoint.{route_id}"}:
        raise RouteMatrixError(f"{route_id} does not end at its required checkpoint")
    if events[-1] != {"type": "shutdown"}:
        raise RouteMatrixError(f"{route_id} does not end with shutdown")
    return RouteContract(
        route_id,
        terminal_id,
        terminal_route_node_id,
        tuple(choice_ids),
        tuple(choice_sequence),
        path,
        len(rows),
        _input_sequence_hash(rows),
    )


def _run_route(
    contract: RouteContract,
    *,
    binary: Path,
    profile: Path,
    package: Path,
    build_identity: Path,
    artifact_root: Path,
    timeout_seconds: int,
) -> dict:
    route_root = artifact_root / contract.route_id
    if route_root.exists():
        raise RouteMatrixError(f"artifact root already contains {contract.route_id}")
    route_root.mkdir(parents=True)
    stdout_path = route_root / "runner.stdout.log"
    stderr_path = route_root / "runner.stderr.log"
    command = [
        str(binary),
        "run",
        "--profile",
        str(profile),
        "--package",
        str(package),
        "--input",
        str(contract.input_path),
        "--artifact-root",
        str(route_root),
        "--build-identity",
        str(build_identity),
    ]
    try:
        # Route traces can be hundreds of megabytes. Stream them directly to
        # private evidence files so parallel coverage never buffers commercial
        # playthrough traces in the matrix coordinator's memory.
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            completed = subprocess.run(
                command,
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=stderr,
                check=False,
                timeout=timeout_seconds,
            )
    except subprocess.TimeoutExpired as error:
        raise RouteMatrixError(f"{contract.route_id} exceeded the route timeout") from error
    if completed.returncode != 0:
        raise RouteMatrixError(f"{contract.route_id} exited with code {completed.returncode}")

    report_path = route_root / "run-report.json"
    report = _load_json(report_path)
    if report.get("schema") != RUN_REPORT_SCHEMA or report.get("status") != "passed":
        raise RouteMatrixError(f"{contract.route_id} did not produce a passing Headless report")
    if report.get("session_id") != f"tsui.{contract.route_id}":
        raise RouteMatrixError(f"{contract.route_id} report session identity does not match")
    profile_value = _load_json(profile)
    if (
        report.get("build_fingerprint") != profile_value.get("build_fingerprint")
        or report.get("package_hash") != profile_value.get("package_hash")
    ):
        raise RouteMatrixError(f"{contract.route_id} report build or package identity does not match")
    if report.get("completed_sequence") != contract.message_count:
        raise RouteMatrixError(f"{contract.route_id} did not consume every physical input message")
    if report.get("input_sequence_hash") != contract.input_sequence_hash:
        raise RouteMatrixError(f"{contract.route_id} report input identity does not match")
    checkpoints = report.get("checkpoint_results")
    if not isinstance(checkpoints, list) or len(checkpoints) != 1:
        raise RouteMatrixError(f"{contract.route_id} report has an invalid checkpoint set")
    checkpoint = checkpoints[0]
    if checkpoint.get("id") != f"checkpoint.{contract.route_id}" or checkpoint.get("passed") is not True:
        raise RouteMatrixError(f"{contract.route_id} required checkpoint did not pass")
    diagnostics = report.get("diagnostics")
    if diagnostics != []:
        raise RouteMatrixError(f"{contract.route_id} report contains diagnostics")

    return {
        "route_id": contract.route_id,
        "terminal_id": contract.terminal_id,
        "terminal_route_node_id": contract.terminal_route_node_id,
        "choice_count": len(contract.choice_ids),
        "choice_selection_count": len(contract.choice_sequence),
        "choice_signature_hash": _json_hash(list(contract.choice_sequence)),
        "session_id": report["session_id"],
        "build_fingerprint": report["build_fingerprint"],
        "package_hash": report["package_hash"],
        "input_sequence_hash": report["input_sequence_hash"],
        "manifest_hash": report["manifest_hash"],
        "completed_sequence": report["completed_sequence"],
        "frame_count": report["frame_count"],
        "audio_frame_count": report["audio_frame_count"],
        "duration_ns": report["duration_ns"],
        "checkpoint_id": checkpoint["id"],
        "checkpoint_observation_hash": checkpoint["observation_hash"],
        "status": "passed",
    }


def _load_resumed_routes(
    path: Path | None,
    contracts: list[RouteContract],
    *,
    build_fingerprint: str,
    package_hash: str,
) -> list[dict]:
    if path is None:
        return []
    report = _load_json(path.resolve(strict=True))
    if report.get("schema") != SCHEMA:
        raise RouteMatrixError("resume report has an invalid schema")
    if report.get("build_fingerprint") != build_fingerprint or report.get("package_hash") != package_hash:
        raise RouteMatrixError("resume report identity does not match this matrix run")
    by_id = {contract.route_id: contract for contract in contracts}
    resumed = report.get("routes")
    if not isinstance(resumed, list):
        raise RouteMatrixError("resume report has an invalid route set")
    validated = []
    seen = set()
    for record in resumed:
        route_id = record.get("route_id") if isinstance(record, dict) else None
        if route_id in seen or route_id not in by_id or record.get("status") != "passed":
            raise RouteMatrixError("resume report contains an invalid passed route")
        contract = by_id[route_id]
        expected = {
            "terminal_id": contract.terminal_id,
            "terminal_route_node_id": contract.terminal_route_node_id,
            "choice_count": len(contract.choice_ids),
            "choice_selection_count": len(contract.choice_sequence),
            "choice_signature_hash": _json_hash(list(contract.choice_sequence)),
            "session_id": f"tsui.{route_id}",
            "build_fingerprint": build_fingerprint,
            "package_hash": package_hash,
            "input_sequence_hash": contract.input_sequence_hash,
            "completed_sequence": contract.message_count,
        }
        if any(record.get(key) != value for key, value in expected.items()):
            raise RouteMatrixError(f"resume evidence identity mismatch for {route_id}")
        seen.add(route_id)
        validated.append(record)
    return validated


def run_matrix(args: argparse.Namespace) -> dict:
    binary = args.binary.resolve(strict=True)
    profile_path = args.profile.resolve(strict=True)
    package = args.package.resolve(strict=True)
    identity_path = args.build_identity.resolve(strict=True)
    automation_root = args.automation_root.resolve(strict=True)
    native_story_ir = _load_json(args.native_story_ir.resolve(strict=True))
    profile = _load_json(profile_path)
    identity = _load_json(identity_path)
    if identity.get("schema") != "astra.build_identity.v1":
        raise RouteMatrixError("build identity has an invalid schema")
    if profile.get("build_fingerprint") != identity.get("identity_hash"):
        raise RouteMatrixError("profile and build identity fingerprints differ")
    if profile.get("package_hash") != _file_hash(package):
        raise RouteMatrixError("profile package hash does not match the package bytes")
    if native_story_ir.get("schema") != "tsuinosora.native_story_ir.v1":
        raise RouteMatrixError("native story IR has an invalid schema")
    routes = native_story_ir.get("routes")
    if not isinstance(routes, list) or not routes:
        raise RouteMatrixError("native story IR contains no route contracts")
    contracts = []
    for route in routes:
        route_id = route.get("route_id") if isinstance(route, dict) else None
        path = automation_root / f"{route_id}.jsonl"
        if not path.is_file():
            raise RouteMatrixError(f"missing generated input sequence for {route_id}")
        contracts.append(_validate_route_input(path, route))
    if len({contract.route_id for contract in contracts}) != len(contracts):
        raise RouteMatrixError("native story IR contains duplicate route ids")

    passed = _load_resumed_routes(
        args.resume_report,
        contracts,
        build_fingerprint=profile["build_fingerprint"],
        package_hash=profile["package_hash"],
    )
    resumed_ids = {item["route_id"] for item in passed}
    pending_contracts = [contract for contract in contracts if contract.route_id not in resumed_ids]

    artifact_root = args.artifact_root.resolve()
    if artifact_root.exists():
        raise RouteMatrixError("matrix artifact root already exists")
    artifact_root.mkdir(parents=True)
    diagnostics = []
    with ThreadPoolExecutor(max_workers=args.jobs, thread_name_prefix="tsui-route") as executor:
        futures = {
            executor.submit(
                _run_route,
                contract,
                binary=binary,
                profile=profile_path,
                package=package,
                build_identity=identity_path,
                artifact_root=artifact_root,
                timeout_seconds=args.timeout_seconds,
            ): contract
            for contract in pending_contracts
        }
        for future in as_completed(futures):
            contract = futures[future]
            try:
                passed.append(future.result())
                print(f"PASS {contract.route_id}", file=sys.stderr, flush=True)
            except Exception as error:
                diagnostics.append(
                    {
                        "code": "TSUI_HEADLESS_ROUTE_FAILED",
                        "route_id": contract.route_id,
                        "message": str(error),
                    }
                )
                print(f"BLOCKED {contract.route_id}", file=sys.stderr, flush=True)
    passed.sort(key=lambda item: item["route_id"])
    diagnostics.sort(key=lambda item: item["route_id"])
    report = {
        "schema": SCHEMA,
        "status": "pass" if len(passed) == len(contracts) else "blocked",
        "build_fingerprint": profile["build_fingerprint"],
        "package_hash": profile["package_hash"],
        "route_count": len(contracts),
        "passed_route_count": len(passed),
        "failed_route_count": len(diagnostics),
        "terminal_observation_required": True,
        "routes": passed,
        "matrix_hash": _json_hash(passed),
        "diagnostics": diagnostics,
        "redaction": {
            "paths": "omitted",
            "payload": "omitted",
            "commercial_text": "omitted",
            "media": "hash_and_count_only",
        },
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--build-identity", type=Path, required=True)
    parser.add_argument("--automation-root", type=Path, required=True)
    parser.add_argument("--native-story-ir", type=Path, required=True)
    parser.add_argument("--artifact-root", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--resume-report", type=Path)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--timeout-seconds", type=int, default=1800)
    return parser


def main() -> int:
    args = _parser().parse_args()
    if args.jobs < 1 or args.jobs > 8:
        raise SystemExit("--jobs must be between 1 and 8")
    if args.timeout_seconds < 1:
        raise SystemExit("--timeout-seconds must be positive")
    try:
        report = run_matrix(args)
    except (OSError, RouteMatrixError) as error:
        print(f"headless route matrix blocked: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, ensure_ascii=False, separators=(",", ":")))
    return 0 if report["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
