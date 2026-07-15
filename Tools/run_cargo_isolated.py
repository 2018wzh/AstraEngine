#!/usr/bin/env python3
"""Run Cargo in a checkout-bound artifact directory and record its identity."""

from __future__ import annotations

import hashlib
import atexit
import contextlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import threading
import time
import uuid
from dataclasses import dataclass
from typing import Iterable


SCHEMA = "astra.build_identity.v1"
REPORT_NAME = "astra-build-identity.json"
GC_SCHEMA = "astra.build_cache_gc.v1"
GC_REPORT_NAME = "astra-build-cache-gc.json"
LEASE_DIRECTORY = ".leases"
PIN_NAME = ".pin"
LAST_USED_NAME = ".last-used"
DEFAULT_GC_MAX_GIB = 50
DEFAULT_GC_KEEP = 3
DEFAULT_GC_MAX_AGE_DAYS = 7
LEASE_STALE_SECONDS = 180
COORDINATOR_STALE_SECONDS = 600


@dataclass(frozen=True)
class CacheEntry:
    path: pathlib.Path
    byte_size: int
    last_used_ns: int
    active: bool
    pinned: bool


class IdentityLease:
    def __init__(self, target: pathlib.Path) -> None:
        self._lease = target / LEASE_DIRECTORY / f"{uuid.uuid4().hex}.json"
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def __enter__(self) -> IdentityLease:
        self._lease.parent.mkdir(parents=True, exist_ok=True)
        self._touch()
        self._thread = threading.Thread(target=self._heartbeat, daemon=True)
        self._thread.start()
        return self

    def _touch(self) -> None:
        payload = {"pid": os.getpid(), "updated_unix_ns": time.time_ns()}
        temporary = self._lease.with_suffix(".partial")
        temporary.write_text(json.dumps(payload, sort_keys=True) + "\n", encoding="utf-8")
        temporary.replace(self._lease)

    def _heartbeat(self) -> None:
        while not self._stop.wait(30):
            try:
                self._touch()
            except OSError:
                return

    def __exit__(self, _type: object, _value: object, _traceback: object) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=5)
        self._lease.unlink(missing_ok=True)
        try:
            self._lease.parent.rmdir()
        except OSError:
            pass


@contextlib.contextmanager
def cache_coordinator(identity_root: pathlib.Path) -> Iterable[None]:
    identity_root.mkdir(parents=True, exist_ok=True)
    lock = identity_root / ".coordinator"
    deadline = time.monotonic() + 60
    while True:
        try:
            lock.mkdir()
            break
        except FileExistsError:
            try:
                stale = time.time_ns() - lock.stat().st_mtime_ns > COORDINATOR_STALE_SECONDS * 1_000_000_000
                if stale:
                    lock.rmdir()
                    continue
            except FileNotFoundError:
                continue
            if time.monotonic() >= deadline:
                raise ValueError("ASTRA_BUILD_CACHE_COORDINATOR_TIMEOUT")
            time.sleep(0.1)
    stop = threading.Event()

    def heartbeat() -> None:
        while not stop.wait(30):
            try:
                lock.touch()
            except OSError:
                return

    thread = threading.Thread(target=heartbeat, daemon=True)
    thread.start()
    try:
        yield
    finally:
        stop.set()
        thread.join(timeout=5)
        try:
            lock.rmdir()
        except FileNotFoundError:
            pass


def _sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def _manifest_hash(root: pathlib.Path) -> str:
    digest = hashlib.sha256()
    manifests: list[pathlib.Path] = []
    ignored_directories = {".git", ".tmp", ".worktrees", "worktrees", "target"}
    for directory, children, files in os.walk(root):
        children[:] = sorted(child for child in children if child not in ignored_directories)
        if "Cargo.toml" in files:
            manifests.append(pathlib.Path(directory) / "Cargo.toml")
    manifests.sort(key=lambda path: path.as_posix())
    for manifest in manifests:
        relative = manifest.relative_to(root).as_posix().encode("utf-8")
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        payload = manifest.read_bytes()
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(payload)
    return "sha256:" + digest.hexdigest()


def _feature_arguments(cargo_args: Iterable[str]) -> list[str]:
    args = list(cargo_args)
    selected: list[str] = []
    index = 0
    value_options = {"--features", "-F", "--target", "--profile"}
    flags = {"--all-features", "--no-default-features", "--release"}
    while index < len(args):
        argument = args[index]
        if argument in value_options:
            selected.append(argument)
            if index + 1 < len(args):
                selected.append(args[index + 1])
                index += 1
        elif argument in flags or argument.startswith("--features="):
            selected.append(argument)
        index += 1
    return selected


def _headless_feature_arguments(cargo_args: Iterable[str]) -> list[str]:
    args = list(cargo_args)
    selected: list[str] = []
    index = 0
    while index < len(args):
        argument = args[index]
        if argument in {"--features", "-F"}:
            if index + 1 < len(args):
                features = [
                    value
                    for value in args[index + 1].split(",")
                    if value == "ffmpeg-vcpkg"
                ]
                if features:
                    selected.extend(["--features", ",".join(features)])
                index += 1
        elif argument.startswith("--features="):
            features = [
                value
                for value in argument.removeprefix("--features=").split(",")
                if value == "ffmpeg-vcpkg"
            ]
            if features:
                selected.append("--features=" + ",".join(features))
        elif argument == "--all-features":
            selected.append("--all-features")
        index += 1
    return selected


def _test_requires_ui_component_fixture(cargo_args: Iterable[str]) -> bool:
    args = list(cargo_args)
    if not args or args[0] != "test" or "--target" in args:
        return False
    if "--workspace" in args:
        return True
    packages: list[str] = []
    index = 0
    while index < len(args):
        if args[index] in {"-p", "--package"} and index + 1 < len(args):
            packages.append(args[index + 1])
            index += 1
        index += 1
    return not packages or "astra-ui-component-host" in packages


def _prepare_ui_component_test_environment(
    *, root: pathlib.Path, cargo_args: list[str], environment: dict[str, str]
) -> None:
    if not _test_requires_ui_component_fixture(cargo_args):
        return
    build = subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "astra-ui-component-host",
            "-p",
            "ui-component-provider",
        ],
        cwd=root,
        env=environment,
        check=False,
    )
    if build.returncode != 0:
        raise ValueError("ASTRA_UI_COMPONENT_FIXTURE_BUILD_FAILED")


def _configure_ffmpeg_runtime(
    cargo_args: Iterable[str], environment: dict[str, str]
) -> None:
    if os.name != "nt" or not _headless_feature_arguments(cargo_args):
        return
    root_value = environment.get("VCPKG_ROOT", "").strip()
    if not root_value:
        raise ValueError("ASTRA_FFMPEG_VCPKG_ROOT_MISSING")
    root = pathlib.Path(root_value)
    triplet = environment.get("VCPKG_DEFAULT_TRIPLET", "x64-windows")
    installed = root / "installed" / triplet
    runtime_directories = [installed / "debug" / "bin", installed / "bin"]
    if not all(path.is_dir() for path in runtime_directories):
        raise ValueError("ASTRA_FFMPEG_VCPKG_RUNTIME_MISSING")
    environment["PATH"] = os.pathsep.join(
        [*(str(path) for path in runtime_directories), environment.get("PATH", "")]
    )


def _prepare_headless_test_environment(
    *, root: pathlib.Path, target: pathlib.Path, report_path: pathlib.Path,
    cargo_args: list[str], environment: dict[str, str]
) -> None:
    _configure_ffmpeg_runtime(cargo_args, environment)
    convergence = subprocess.run(
        [sys.executable, str(root / "Tools" / "check_headless_test_convergence.py")],
        cwd=root,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if convergence.returncode != 0:
        raise ValueError("ASTRA_HEADLESS_TEST_CONVERGENCE_FAILED: " + convergence.stdout.strip())
    environment["ASTRA_HEADLESS_TEST_INVENTORY"] = convergence.stdout.strip()
    shipping = subprocess.run(
        [sys.executable, str(root / "Tools" / "check_headless_shipping_graph.py")],
        cwd=root,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=environment,
    )
    if shipping.returncode != 0:
        raise ValueError("ASTRA_HEADLESS_SHIPPING_GRAPH_FAILED: " + shipping.stdout.strip())
    environment["ASTRA_HEADLESS_SHIPPING_GRAPH"] = shipping.stdout.strip()
    build_command = ["cargo", "build", "-p", "astra-headless"]
    build_command.extend(_headless_feature_arguments(cargo_args))
    build = subprocess.run(build_command, cwd=root, env=environment, check=False)
    if build.returncode != 0:
        raise ValueError("ASTRA_HEADLESS_DRIVER_BUILD_FAILED")
    suffix = ".exe" if os.name == "nt" else ""
    built_binary = target / "debug" / f"astra-headless{suffix}"
    if not built_binary.is_file():
        raise ValueError("ASTRA_HEADLESS_DRIVER_BINARY_MISSING")
    test_root = target / "headless-test-environment"
    driver_root = test_root / "driver"
    driver_root.mkdir(parents=True, exist_ok=True)
    binary = driver_root / f"astra-headless{suffix}"
    shutil.copy2(built_binary, binary)
    binary_hash = _sha256(binary.read_bytes())
    build_identity_report = json.loads(report_path.read_text(encoding="utf-8"))
    environment["ASTRA_HEADLESS_DRIVER_IDENTITY"] = json.dumps(
        {
            "schema": "astra.headless_driver_identity.v1",
            "role": "developer_test_backend",
            "relative_path": f"headless-test-environment/driver/astra-headless{suffix}",
            "binary_hash": binary_hash,
            "build_identity_hash": environment["ASTRA_BUILD_IDENTITY_HASH"],
            "toolchain_fingerprint": build_identity_report["toolchain_fingerprint"],
            "feature_fingerprint": build_identity_report["feature_fingerprint"],
            "dependency_lock_hash": build_identity_report["dependency_lock_hash"],
            "workspace_manifest_hash": build_identity_report["workspace_manifest_hash"],
        },
        sort_keys=True,
    )
    artifact_root = test_root / "artifacts"
    if artifact_root.exists():
        shutil.rmtree(artifact_root)
    bootstrap = subprocess.run(
        [
            str(binary),
            "bootstrap-test-env",
            "--output",
            str(test_root),
            "--build-identity",
            str(report_path),
        ],
        cwd=root,
        env=environment,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if bootstrap.returncode != 0:
        raise ValueError(
            "ASTRA_HEADLESS_TEST_ENVIRONMENT_FAILED: " + bootstrap.stderr.strip()
        )
    environment["ASTRA_HEADLESS_BINARY"] = str(binary)
    environment["ASTRA_HEADLESS_BINARY_HASH"] = binary_hash
    environment["ASTRA_HEADLESS_PROFILE"] = str(test_root / "headless-profile.json")
    environment["ASTRA_HEADLESS_PACKAGE"] = str(test_root / "empty.astrapkg")
    environment["ASTRA_HEADLESS_ARTIFACT_ROOT"] = str(artifact_root)
    environment["ASTRA_BUILD_IDENTITY"] = str(report_path)


def build_identity(
    *,
    root: pathlib.Path,
    git_head: str,
    git_diff: bytes,
    rustc_version: str,
    cargo_args: Iterable[str],
    untracked_files: Iterable[tuple[str, bytes]] = (),
    ui_toolchain: dict[str, object] | None = None,
) -> dict[str, object]:
    root = root.resolve()
    lock_path = root / "Cargo.lock"
    checkout_digest = hashlib.sha256()
    checkout_digest.update(git_head.strip().encode("utf-8"))
    checkout_digest.update(b"\0")
    checkout_digest.update(git_diff)
    for relative, payload in sorted(untracked_files, key=lambda item: item[0]):
        encoded = relative.encode("utf-8")
        checkout_digest.update(len(encoded).to_bytes(8, "big"))
        checkout_digest.update(encoded)
        checkout_digest.update(len(payload).to_bytes(8, "big"))
        checkout_digest.update(payload)
    identity: dict[str, object] = {
        "schema": SCHEMA,
        "checkout_id": git_head.strip(),
        "checkout_state_hash": "sha256:" + checkout_digest.hexdigest(),
        "workspace_manifest_hash": _manifest_hash(root),
        "dependency_lock_hash": _sha256(lock_path.read_bytes()),
        "toolchain_fingerprint": _sha256(rustc_version.encode("utf-8")),
        "ui_toolchain": ui_toolchain or {"status": "not_configured"},
        "feature_fingerprint": _sha256(
            json.dumps(
                _feature_arguments(cargo_args),
                ensure_ascii=True,
                separators=(",", ":"),
            ).encode("utf-8")
        ),
        "artifact_path_role": "checkout_bound_cargo_target",
    }
    canonical = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode("utf-8")
    identity["identity_hash"] = _sha256(canonical)
    return identity


def target_directory(root: pathlib.Path, identity: dict[str, object]) -> pathlib.Path:
    identity_hash = str(identity["identity_hash"])
    if not identity_hash.startswith("sha256:") or len(identity_hash) != 71:
        raise ValueError("identity_hash must be a sha256 value")
    return root / "target" / "identity" / identity_hash.removeprefix("sha256:")[:16]


def collect_artifacts(target: pathlib.Path) -> list[dict[str, object]]:
    suffix_roles = {
        ".exe": "executable",
        ".dll": "dynamic_library",
        ".so": "dynamic_library",
        ".dylib": "dynamic_library",
    }
    artifacts: list[dict[str, object]] = []
    if not target.exists():
        return artifacts
    for profile in sorted(path for path in target.iterdir() if path.is_dir()):
        for artifact in sorted(path for path in profile.iterdir() if path.is_file()):
            role = suffix_roles.get(artifact.suffix.lower())
            if role is None:
                continue
            payload = artifact.read_bytes()
            artifacts.append(
                {
                    "path": artifact.relative_to(target).as_posix(),
                    "role": role,
                    "sha256": _sha256(payload),
                    "byte_size": len(payload),
                }
            )
    return artifacts


def validate_existing_identity(
    report_path: pathlib.Path, identity: dict[str, object]
) -> None:
    if not report_path.exists():
        return
    try:
        existing = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"ASTRA_BUILD_IDENTITY_INVALID: {error}") from error
    if existing.get("identity_hash") != identity.get("identity_hash"):
        raise ValueError(
            "ASTRA_BUILD_IDENTITY_MISMATCH: target directory belongs to another build identity"
        )


def ensure_identity_unchanged(
    before: dict[str, object], after: dict[str, object]
) -> None:
    if before.get("identity_hash") != after.get("identity_hash"):
        raise ValueError(
            "ASTRA_BUILD_INPUT_CHANGED: checkout or build inputs changed while Cargo was running"
        )


def _run_output(command: list[str], root: pathlib.Path) -> bytes:
    return subprocess.run(
        command,
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout


def _untracked_files(root: pathlib.Path) -> list[tuple[str, bytes]]:
    raw = _run_output(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"], root
    )
    files: list[tuple[str, bytes]] = []
    for encoded in raw.split(b"\0"):
        if not encoded:
            continue
        relative = encoded.decode("utf-8")
        path = root / relative
        if path.is_file():
            files.append((pathlib.PurePath(relative).as_posix(), path.read_bytes()))
    return files


def _current_identity(root: pathlib.Path, cargo_args: Iterable[str]) -> dict[str, object]:
    git_head = _run_output(["git", "rev-parse", "HEAD"], root).decode("utf-8").strip()
    git_diff = _run_output(["git", "diff", "--binary", "HEAD"], root)
    rustc_version = _run_output(["rustc", "-Vv"], root).decode("utf-8")
    return build_identity(
        root=root,
        git_head=git_head,
        git_diff=git_diff,
        rustc_version=rustc_version,
        cargo_args=cargo_args,
        untracked_files=_untracked_files(root),
        ui_toolchain=_ui_toolchain_identity(root),
    )


def _ui_toolchain_identity(root: pathlib.Path) -> dict[str, object] | None:
    lock_path = root / "Tools" / "ui-toolchain-lock.json"
    if not lock_path.is_file():
        return None
    report_path = root / ".tmp" / "ui-toolchain" / "preflight.json"
    if not report_path.is_file():
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_MISSING")
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"ASTRA_UI_TOOLCHAIN_PREFLIGHT_INVALID: {error}") from error
    if report.get("schema") != "astra.ui_toolchain_preflight.v1":
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_SCHEMA_INVALID")
    if report.get("lock_sha256") != _sha256(lock_path.read_bytes()):
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_STALE")
    if report.get("controller_analysis") != "passed":
        raise ValueError("ASTRA_UI_TOOLCHAIN_LUAU_ANALYSIS_NOT_PASSED")
    tools = report.get("tools")
    if not isinstance(tools, dict) or set(tools) != {"node", "luau_analyze", "jco"}:
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_TOOLS_INVALID")
    claimed_hash = report.pop("report_hash", None)
    canonical = json.dumps(report, sort_keys=True, separators=(",", ":")).encode("utf-8")
    actual_hash = _sha256(canonical)
    if claimed_hash != actual_hash:
        raise ValueError("ASTRA_UI_TOOLCHAIN_PREFLIGHT_HASH_INVALID")
    return {
        "lock_sha256": report["lock_sha256"],
        "target": report.get("target"),
        "tools": tools,
        "supply_chain": report.get("supply_chain"),
        "preflight_hash": actual_hash,
    }


def _prepare_ui_toolchain(root: pathlib.Path) -> None:
    lock_path = root / "Tools" / "ui-toolchain-lock.json"
    if not lock_path.is_file():
        return
    command = [
        sys.executable,
        str(root / "Tools" / "bootstrap_ui_toolchain.py"),
        "--output",
        str(root / ".tmp" / "ui-toolchain" / "preflight.json"),
    ]
    completed = subprocess.run(command, cwd=root, text=True, capture_output=True)
    if completed.returncode:
        raise ValueError(
            "ASTRA_UI_TOOLCHAIN_PREFLIGHT_FAILED: " + completed.stderr.strip()
        )


def _write_report(path: pathlib.Path, report: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".partial")
    temporary.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def _environment_integer(name: str, default: int, *, minimum: int) -> int:
    raw = os.environ.get(name)
    if raw is None:
        return default
    try:
        value = int(raw)
    except ValueError as error:
        raise ValueError(f"ASTRA_BUILD_CACHE_CONFIG_INVALID: {name} must be an integer") from error
    if value < minimum:
        raise ValueError(
            f"ASTRA_BUILD_CACHE_CONFIG_INVALID: {name} must be at least {minimum}"
        )
    return value


def _directory_size(path: pathlib.Path) -> int:
    total = 0
    for directory, _children, files in os.walk(path):
        for filename in files:
            candidate = pathlib.Path(directory) / filename
            try:
                if not candidate.is_symlink():
                    total += candidate.stat().st_size
            except FileNotFoundError:
                continue
    return total


def _identity_is_active(path: pathlib.Path, now_ns: int) -> bool:
    lease_root = path / LEASE_DIRECTORY
    if not lease_root.is_dir():
        return False
    active = False
    stale_before = now_ns - LEASE_STALE_SECONDS * 1_000_000_000
    for lease in lease_root.glob("*.json"):
        try:
            if lease.stat().st_mtime_ns >= stale_before:
                active = True
            else:
                lease.unlink(missing_ok=True)
        except FileNotFoundError:
            continue
    return active


def _cache_entries(identity_root: pathlib.Path, now_ns: int) -> list[CacheEntry]:
    entries: list[CacheEntry] = []
    if not identity_root.is_dir():
        return entries
    for path in identity_root.iterdir():
        if not path.is_dir() or path.name.startswith("."):
            continue
        marker = path / LAST_USED_NAME
        report = path / REPORT_NAME
        try:
            timestamp_source = marker if marker.is_file() else report if report.is_file() else path
            last_used_ns = timestamp_source.stat().st_mtime_ns
        except FileNotFoundError:
            continue
        entries.append(
            CacheEntry(
                path=path,
                byte_size=_directory_size(path),
                last_used_ns=last_used_ns,
                active=_identity_is_active(path, now_ns),
                pinned=(path / PIN_NAME).is_file(),
            )
        )
    return entries


def collect_cache(
    *, identity_root: pathlib.Path, current: pathlib.Path | None, max_bytes: int,
    keep: int, max_age_days: int
) -> dict[str, object]:
    now_ns = time.time_ns()
    recycle_root = identity_root / ".recycle"
    if recycle_root.exists():
        shutil.rmtree(recycle_root)
    entries = _cache_entries(identity_root, now_ns)
    newest = sorted(entries, key=lambda entry: entry.last_used_ns, reverse=True)
    protected = {entry.path for entry in newest[:keep]}
    if current is not None:
        protected.add(current)
    age_cutoff = now_ns - max_age_days * 86_400 * 1_000_000_000
    total_before = sum(entry.byte_size for entry in entries)
    remaining = total_before
    removed: list[dict[str, object]] = []
    skipped: list[dict[str, str]] = []
    candidates = sorted(entries, key=lambda entry: entry.last_used_ns)
    for entry in candidates:
        reason = None
        if entry.path in protected:
            reason = "retained"
        elif entry.active:
            reason = "active"
        elif entry.pinned:
            reason = "pinned"
        if reason is not None:
            skipped.append({"identity": entry.path.name, "reason": reason})
            continue
        expired = entry.last_used_ns < age_cutoff
        over_budget = remaining > max_bytes
        if not expired and not over_budget:
            continue
        # Recheck immediately before the atomic rename so a newly active build is preserved.
        if _identity_is_active(entry.path, time.time_ns()):
            skipped.append({"identity": entry.path.name, "reason": "active"})
            continue
        recycle_root.mkdir(parents=True, exist_ok=True)
        recycled = recycle_root / f"{entry.path.name}-{uuid.uuid4().hex}"
        entry.path.replace(recycled)
        shutil.rmtree(recycled)
        remaining -= entry.byte_size
        removed.append(
            {
                "identity": entry.path.name,
                "byte_size": entry.byte_size,
                "reason": "expired" if expired else "capacity",
            }
        )
    report: dict[str, object] = {
        "schema": GC_SCHEMA,
        "status": "pass",
        "max_byte_size": max_bytes,
        "keep_count": keep,
        "max_age_days": max_age_days,
        "byte_size_before": total_before,
        "byte_size_after": remaining,
        "removed": removed,
        "skipped": skipped,
    }
    _write_report(identity_root / GC_REPORT_NAME, report)
    return report


def _gc_configuration() -> tuple[int, int, int]:
    max_gib = _environment_integer("ASTRA_CARGO_CACHE_MAX_GIB", DEFAULT_GC_MAX_GIB, minimum=1)
    keep = _environment_integer("ASTRA_CARGO_CACHE_KEEP", DEFAULT_GC_KEEP, minimum=0)
    max_age_days = _environment_integer(
        "ASTRA_CARGO_CACHE_MAX_AGE_DAYS", DEFAULT_GC_MAX_AGE_DAYS, minimum=1
    )
    return max_gib * 1024**3, keep, max_age_days


def _run_gc(root: pathlib.Path, current: pathlib.Path | None) -> dict[str, object]:
    max_bytes, keep, max_age_days = _gc_configuration()
    return collect_cache(
        identity_root=root / "target" / "identity",
        current=current,
        max_bytes=max_bytes,
        keep=keep,
        max_age_days=max_age_days,
    )


def main(argv: list[str]) -> int:
    auto_gc = True
    if argv and argv[0] == "--no-auto-gc":
        auto_gc = False
        argv = argv[1:]
    if argv == ["--gc-only"]:
        root = pathlib.Path(__file__).resolve().parents[1]
        try:
            with cache_coordinator(root / "target" / "identity"):
                report = _run_gc(root, current=None)
        except (OSError, ValueError) as error:
            print(f"ASTRA_BUILD_CACHE_GC_FAILED: {error}", file=sys.stderr)
            return 2
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
        return 0
    if not argv:
        print(
            "usage: python Tools/run_cargo_isolated.py [--no-auto-gc] <cargo arguments>\n"
            "       python Tools/run_cargo_isolated.py --gc-only",
            file=sys.stderr,
        )
        return 2
    root = pathlib.Path(__file__).resolve().parents[1]
    try:
        _prepare_ui_toolchain(root)
        identity = _current_identity(root, argv)
    except (OSError, subprocess.CalledProcessError, UnicodeDecodeError, ValueError) as error:
        print(f"ASTRA_BUILD_IDENTITY_FAILED: {error}", file=sys.stderr)
        return 2

    target = target_directory(root, identity)
    lease = IdentityLease(target)
    try:
        with cache_coordinator(root / "target" / "identity"):
            lease.__enter__()
            atexit.register(lease.__exit__, None, None, None)
            if auto_gc:
                _run_gc(root, current=target)
    except (OSError, ValueError) as error:
        lease.__exit__(None, None, None)
        print(f"ASTRA_BUILD_CACHE_GC_FAILED: {error}", file=sys.stderr)
        return 2
    (target / LAST_USED_NAME).touch()
    report_path = target / REPORT_NAME
    try:
        validate_existing_identity(report_path, identity)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 2
    report = dict(identity)
    report["status"] = "running"
    report["artifacts"] = []
    _write_report(report_path, report)

    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target)
    environment["ASTRA_BUILD_IDENTITY_HASH"] = str(identity["identity_hash"])
    if argv[0] == "test":
        try:
            _prepare_headless_test_environment(
                root=root,
                target=target,
                report_path=report_path,
                cargo_args=argv,
                environment=environment,
            )
            _prepare_ui_component_test_environment(
                root=root, cargo_args=argv, environment=environment
            )
        except ValueError as error:
            report["status"] = "blocked"
            report["headless_driver_error"] = str(error)
            _write_report(report_path, report)
            print(str(error), file=sys.stderr)
            return 2
        report["headless_driver_identity"] = json.loads(
            environment["ASTRA_HEADLESS_DRIVER_IDENTITY"]
        )
        _write_report(report_path, report)
    result = subprocess.run(["cargo", *argv], cwd=root, env=environment, check=False)

    report["artifacts"] = collect_artifacts(target)
    report["cargo_exit_code"] = result.returncode
    if argv[0] == "test":
        inventory = environment.get("ASTRA_HEADLESS_TEST_INVENTORY")
        if inventory:
            report["headless_test_inventory"] = json.loads(inventory)
        shipping = environment.get("ASTRA_HEADLESS_SHIPPING_GRAPH")
        if shipping:
            report["headless_shipping_graph"] = json.loads(shipping)
        session_reports = sorted(
            (target / "headless-test-environment" / "artifacts").glob("test-*/test-*.run-report.json")
        )
        session_payloads = [json.loads(path.read_text(encoding="utf-8")) for path in session_reports]
        report["headless_sessions"] = session_payloads
        session_violations = []
        for path, payload in zip(session_reports, session_payloads, strict=True):
            manifest_path = path.parent / "artifact-manifest.json"
            manifest_hash = _sha256(manifest_path.read_bytes()) if manifest_path.is_file() else ""
            if (
                payload.get("schema") != "astra.headless_run_report.v1"
                or payload.get("status") != "passed"
                or payload.get("diagnostics")
                or not str(payload.get("manifest_hash", "")).startswith("sha256:")
                or not str(payload.get("build_fingerprint", "")).startswith("sha256:")
                or not str(payload.get("package_hash", "")).startswith("sha256:")
                or not str(payload.get("input_sequence_hash", "")).startswith("sha256:")
                or manifest_hash != payload.get("manifest_hash")
                or payload.get("session_id") != path.parent.name
            ):
                session_violations.append(
                    {
                        "session_id": payload.get("session_id", "unknown"),
                        "code": "ASTRA_HEADLESS_TEST_SESSION_INVALID",
                    }
                )
        report["headless_executed_session_count"] = len(session_payloads)
        if result.returncode == 0 and "--workspace" in argv and not session_payloads:
            session_violations.append(
                {"code": "ASTRA_HEADLESS_TEST_SESSION_INVENTORY_EMPTY"}
            )
        report["headless_session_violations"] = session_violations
    try:
        ensure_identity_unchanged(identity, _current_identity(root, argv))
    except (OSError, subprocess.CalledProcessError, UnicodeDecodeError, ValueError) as error:
        report["status"] = "blocked"
        report["diagnostics"] = ["ASTRA_BUILD_INPUT_CHANGED"]
        _write_report(report_path, report)
        print(str(error), file=sys.stderr)
        return 2
    headless_failed = bool(report.get("headless_session_violations"))
    report["status"] = "pass" if result.returncode == 0 and not headless_failed else "failed"
    _write_report(report_path, report)
    (target / LAST_USED_NAME).touch()
    return 2 if result.returncode == 0 and headless_failed else result.returncode


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
