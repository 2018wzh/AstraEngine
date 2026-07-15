#!/usr/bin/env python3
"""Validate and provision the pinned Migration 12 UI build toolchain."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile


SCHEMA = "astra.ui_toolchain_preflight.v1"


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def target_name() -> str:
    systems = {"Windows": "windows", "Linux": "linux", "Darwin": "macos"}
    machines = {"AMD64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}
    system = systems.get(platform.system())
    machine = machines.get(platform.machine())
    if not system or not machine:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_TARGET_UNSUPPORTED")
    return f"{system}-{machine}"


def run(command: list[str], *, cwd: pathlib.Path) -> str:
    completed = subprocess.run(
        command, cwd=cwd, text=True, encoding="utf-8", errors="strict", capture_output=True
    )
    if completed.returncode:
        raise RuntimeError(
            "ASTRA_UI_TOOLCHAIN_COMMAND_FAILED: "
            + " ".join(command[:2])
            + "\n"
            + completed.stderr.strip()
        )
    return completed.stdout.strip()


def validate_lock(lock: dict[str, object]) -> None:
    if lock.get("schema") != "astra.ui_toolchain_lock.v1":
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_LOCK_SCHEMA_INVALID")
    for tool in ("node", "luau", "jco"):
        if not isinstance(lock.get(tool), dict):
            raise RuntimeError(f"ASTRA_UI_TOOLCHAIN_LOCK_MISSING: {tool}")


def provision_luau(root: pathlib.Path, lock: dict[str, object], target: str) -> pathlib.Path:
    luau = lock["luau"]
    assert isinstance(luau, dict)
    targets = luau.get("targets")
    if not isinstance(targets, dict) or target not in targets:
        raise RuntimeError(f"ASTRA_UI_TOOLCHAIN_TARGET_MISSING: luau/{target}")
    config = targets[target]
    assert isinstance(config, dict)
    cache = root / ".tmp" / "ui-toolchain" / "downloads"
    install = root / ".tmp" / "ui-toolchain" / f"luau-{luau['version']}-{target}"
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / pathlib.PurePosixPath(str(config["url"])).name
    if not archive.exists() or sha256(archive) != config["archive_sha256"]:
        temporary = archive.with_suffix(archive.suffix + ".partial")
        with urllib.request.urlopen(str(config["url"]), timeout=60) as response, temporary.open("wb") as stream:
            shutil.copyfileobj(response, stream)
        if sha256(temporary) != config["archive_sha256"]:
            temporary.unlink(missing_ok=True)
            raise RuntimeError("ASTRA_UI_TOOLCHAIN_ARCHIVE_HASH_MISMATCH: luau")
        temporary.replace(archive)
    analyzer = install / str(config["analyzer_path"])
    if not analyzer.exists() or sha256(analyzer) != config["analyzer_sha256"]:
        with tempfile.TemporaryDirectory(dir=install.parent) as temporary_name:
            temporary = pathlib.Path(temporary_name)
            with zipfile.ZipFile(archive) as bundle:
                for member in bundle.infolist():
                    destination = (temporary / member.filename).resolve()
                    if temporary.resolve() not in destination.parents and destination != temporary.resolve():
                        raise RuntimeError("ASTRA_UI_TOOLCHAIN_ARCHIVE_PATH_INVALID")
                bundle.extractall(temporary)
            extracted = temporary / str(config["analyzer_path"])
            if not extracted.is_file() or sha256(extracted) != config["analyzer_sha256"]:
                raise RuntimeError("ASTRA_UI_TOOLCHAIN_EXECUTABLE_HASH_MISMATCH: luau-analyze")
            shutil.rmtree(install, ignore_errors=True)
            temporary.replace(install)
    return analyzer


def provision_jco(root: pathlib.Path, lock: dict[str, object]) -> tuple[pathlib.Path, dict[str, object]]:
    source = root / "Tools" / "ui-component-web"
    package_lock = source / "package-lock.json"
    if not package_lock.is_file():
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_NPM_LOCK_MISSING")
    npm = shutil.which("npm")
    if npm is None:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_NPM_MISSING")
    run([npm, "ci", "--ignore-scripts", "--no-audit", "--no-fund"], cwd=source)
    package = source / "node_modules" / "@bytecodealliance" / "jco" / "package.json"
    metadata = json.loads(package.read_text(encoding="utf-8"))
    jco_lock = lock["jco"]
    assert isinstance(jco_lock, dict)
    if metadata.get("version") != jco_lock["version"] or metadata.get("license") != jco_lock["license"]:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_JCO_IDENTITY_MISMATCH")
    npm_lock = json.loads(package_lock.read_text(encoding="utf-8"))
    dependency = npm_lock.get("packages", {}).get("node_modules/@bytecodealliance/jco", {})
    if dependency.get("integrity") != jco_lock["integrity"]:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_JCO_INTEGRITY_MISMATCH")
    executable = source / "node_modules" / ".bin" / ("jco.cmd" if os.name == "nt" else "jco")
    if not executable.is_file():
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_JCO_EXECUTABLE_MISSING")
    return executable, {"package_lock_sha256": sha256(package_lock), "package_sha256": sha256(package)}


def analyze_luau(root: pathlib.Path, analyzer: pathlib.Path) -> None:
    definitions = root / "Tools" / "LuauTypes" / "astra-ui.d.luau"
    controllers = sorted((root / "Examples" / "NativeVN" / "Controllers").glob("*.luau"))
    if not controllers:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_CONTROLLER_SOURCES_MISSING")
    harness_root = root / ".tmp" / "ui-toolchain" / "luau-analysis"
    harness_root.mkdir(parents=True, exist_ok=True)
    definition_source = definitions.read_text(encoding="utf-8")
    for controller in controllers:
        harness = harness_root / f"{controller.stem}.analysis.luau"
        harness.write_text(
            definition_source + "\n" + controller.read_text(encoding="utf-8"),
            encoding="utf-8",
        )
        run([str(analyzer), "--mode=strict", str(harness)], cwd=root)


def supply_chain_preflight(root: pathlib.Path, target: str) -> dict[str, object]:
    metadata = json.loads(run(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=root))
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise RuntimeError("ASTRA_UI_SUPPLY_CHAIN_METADATA_INVALID")
    by_name: dict[str, list[dict[str, object]]] = {}
    for package in packages:
        if isinstance(package, dict) and isinstance(package.get("name"), str):
            by_name.setdefault(str(package["name"]), []).append(package)
    forbidden = sorted(name for name in ("yakui-wgpu", "yakui-winit", "yakui-app") if name in by_name)
    if forbidden:
        raise RuntimeError("ASTRA_UI_SUPPLY_CHAIN_FORBIDDEN: " + ",".join(forbidden))
    expected = {
        "cosmic-text": ("0.18.2", "registry+https://github.com/rust-lang/crates.io-index"),
        "yakui-core": ("0.3.0", "git+https://github.com/SecondHalfGames/yakui?rev=4b87f09ce18c36975de022507123eb360153728d#4b87f09ce18c36975de022507123eb360153728d"),
        "yakui-widgets": ("0.3.0", "git+https://github.com/SecondHalfGames/yakui?rev=4b87f09ce18c36975de022507123eb360153728d#4b87f09ce18c36975de022507123eb360153728d"),
    }
    evidence: dict[str, object] = {}
    for name, (version, source) in expected.items():
        matches = by_name.get(name, [])
        if len(matches) != 1:
            raise RuntimeError(f"ASTRA_UI_SUPPLY_CHAIN_DUPLICATE_OR_MISSING: {name}")
        package = matches[0]
        if package.get("version") != version or package.get("source") != source:
            raise RuntimeError(f"ASTRA_UI_SUPPLY_CHAIN_IDENTITY_MISMATCH: {name}")
        if package.get("license") != "MIT OR Apache-2.0":
            raise RuntimeError(f"ASTRA_UI_SUPPLY_CHAIN_LICENSE_REJECTED: {name}")
        evidence[name] = {
            "version": version,
            "source_sha256": "sha256:" + hashlib.sha256(source.encode()).hexdigest(),
            "license": package["license"],
        }
    if target == "windows-x86_64" and "windows" not in by_name:
        raise RuntimeError("ASTRA_UI_SUPPLY_CHAIN_WINDOWS_UIA_DEPENDENCY_MISSING")
    installed_targets = set(run(["rustup", "target", "list", "--installed"], cwd=root).splitlines())
    required_targets = {"wasm32-unknown-unknown"}
    if target == "windows-x86_64":
        required_targets.add("x86_64-pc-windows-msvc")
    missing_targets = sorted(required_targets - installed_targets)
    if missing_targets:
        raise RuntimeError("ASTRA_UI_SUPPLY_CHAIN_RUST_TARGET_MISSING: " + ",".join(missing_targets))
    return {
        "packages": evidence,
        "rust_targets": sorted(required_targets),
        "forbidden_packages": [],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--skip-analysis", action="store_true")
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parents[1]
    lock_path = root / "Tools" / "ui-toolchain-lock.json"
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    validate_lock(lock)
    target = target_name()

    node = shutil.which("node")
    if node is None:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_NODE_MISSING")
    node_path = pathlib.Path(node).resolve()
    node_lock = lock["node"]
    assert isinstance(node_lock, dict)
    node_targets = node_lock.get("targets")
    if not isinstance(node_targets, dict) or target not in node_targets:
        raise RuntimeError(f"ASTRA_UI_TOOLCHAIN_TARGET_MISSING: node/{target}")
    actual_version = run([str(node_path), "--version"], cwd=root).removeprefix("v")
    if actual_version != node_lock["version"]:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_NODE_VERSION_MISMATCH")
    node_target = node_targets[target]
    assert isinstance(node_target, dict)
    if sha256(node_path) != node_target["executable_sha256"]:
        raise RuntimeError("ASTRA_UI_TOOLCHAIN_NODE_HASH_MISMATCH")

    analyzer = provision_luau(root, lock, target)
    jco, jco_evidence = provision_jco(root, lock)
    if not args.skip_analysis:
        analyze_luau(root, analyzer)
    supply_chain = supply_chain_preflight(root, target)
    report: dict[str, object] = {
        "schema": SCHEMA,
        "target": target,
        "lock_sha256": sha256(lock_path),
        "tools": {
            "node": {"version": actual_version, "executable_sha256": sha256(node_path)},
            "luau_analyze": {"version": lock["luau"]["version"], "executable_sha256": sha256(analyzer)},
            "jco": {"version": lock["jco"]["version"], **jco_evidence},
        },
        "controller_analysis": "skipped" if args.skip_analysis else "passed",
        "supply_chain": supply_chain,
    }
    canonical = json.dumps(report, sort_keys=True, separators=(",", ":")).encode()
    report["report_hash"] = "sha256:" + hashlib.sha256(canonical).hexdigest()
    output = args.output or root / ".tmp" / "ui-toolchain" / "preflight.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(".partial")
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(output)
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
