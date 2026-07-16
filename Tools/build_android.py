#!/usr/bin/env python3
"""Build and identify the Astra Android Player without using shared Cargo artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parents[1]
ANDROID_PROJECT = ROOT / "Engine/Source/Programs/astra-player-android/android"
SCHEMA = "astra.android_bundle_manifest.v1"
NDK_VERSION = "30.0.15729638"
GRADLE_VERSION = "9.5.0"
AGP_VERSION = "9.3.0"
BUILD_TOOLS = "36.0.0"
SDK_API = 36


class BuildError(RuntimeError):
    pass


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return f"sha256:{digest.hexdigest()}"


def run(command: list[str], *, cwd: pathlib.Path = ROOT, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    if result.returncode:
        raise BuildError(f"ASTRA_ANDROID_COMMAND_FAILED: {' '.join(command)}\n{result.stdout}")
    return result.stdout


def require_tool(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise BuildError(f"ASTRA_ANDROID_TOOL_MISSING: {name}")
    return path


def sdk_root() -> pathlib.Path:
    value = os.environ.get("ANDROID_SDK_ROOT") or os.environ.get("ANDROID_HOME")
    if not value:
        raise BuildError("ASTRA_ANDROID_SDK_ROOT_MISSING")
    root = pathlib.Path(value).resolve()
    for relative in (
        f"platforms/android-{SDK_API}",
        f"build-tools/{BUILD_TOOLS}",
        f"ndk/{NDK_VERSION}",
    ):
        if not (root / relative).is_dir():
            raise BuildError(f"ASTRA_ANDROID_SDK_COMPONENT_MISSING: {relative}")
    return root


def ndk_prebuilt(sdk: pathlib.Path) -> pathlib.Path:
    prebuilt_root = sdk / f"ndk/{NDK_VERSION}/toolchains/llvm/prebuilt"
    prebuilt = [entry for entry in prebuilt_root.iterdir() if entry.is_dir()]
    if len(prebuilt) != 1:
        raise BuildError("ASTRA_ANDROID_NDK_PREBUILT_AMBIGUOUS")
    return prebuilt[0]


def copy_cxx_runtime(sdk: pathlib.Path, jni_dir: pathlib.Path, abis: list[str]) -> None:
    prebuilt = ndk_prebuilt(sdk)
    triples = {
        "arm64-v8a": "aarch64-linux-android",
        "x86_64": "x86_64-linux-android",
    }
    for abi in abis:
        source = prebuilt / f"sysroot/usr/lib/{triples[abi]}/libc++_shared.so"
        if not source.is_file():
            raise BuildError(f"ASTRA_ANDROID_CXX_RUNTIME_MISSING: {abi}")
        destination = jni_dir / abi / source.name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        if sha256(source) != sha256(destination):
            raise BuildError(f"ASTRA_ANDROID_CXX_RUNTIME_COPY_MISMATCH: {abi}")


def validate_java() -> tuple[str, pathlib.Path]:
    java = pathlib.Path(require_tool("java")).resolve()
    output = subprocess.run(
        [java, "-version"], check=False, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
    ).stdout
    match = re.search(r'version "([^"]+)"', output)
    if match is None or int(match.group(1).split(".", 1)[0]) != 17:
        raise BuildError("ASTRA_ANDROID_JDK_MISMATCH: JDK 17 is required")
    return match.group(1), java


def git_identity() -> tuple[str, bool]:
    commit = run(["git", "rev-parse", "HEAD"]).strip()
    dirty = bool(run(["git", "status", "--porcelain"]).strip())
    return commit, dirty


def build_fingerprint(
    package: pathlib.Path,
    abis: list[str],
    features: list[str],
    jdk_version: str,
    toolchain: list[dict[str, str]],
) -> str:
    commit, dirty = git_identity()
    identity = {
        "schema": "astra.android_build_identity.v1",
        "commit": commit,
        "dirty": dirty,
        "cargo_lock_hash": sha256(ROOT / "Cargo.lock"),
        "package_hash": sha256(package),
        "sdk": SDK_API,
        "build_tools": BUILD_TOOLS,
        "ndk": NDK_VERSION,
        "gradle": GRADLE_VERSION,
        "agp": AGP_VERSION,
        "jdk": jdk_version,
        "toolchain": toolchain,
        "abis": abis,
        "features": features,
    }
    encoded = json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    return f"sha256:{hashlib.sha256(encoded).hexdigest()}"


def artifact(kind: str, path: pathlib.Path) -> dict[str, str]:
    if not path.is_file():
        raise BuildError(f"ASTRA_ANDROID_IDENTITY_INPUT_MISSING: {kind}")
    return {"kind": kind, "file_name": path.name, "sha256": sha256(path)}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", required=True, type=pathlib.Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--profile", default="android-release")
    parser.add_argument("--application-id", required=True)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--with-emulator-abi", action="store_true")
    parser.add_argument("--signing-properties", type=pathlib.Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    package = args.package.resolve()
    if not package.is_file() or package.suffix != ".astrapkg":
        raise BuildError("ASTRA_ANDROID_PACKAGE_INVALID")
    if not re.fullmatch(r"[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+", args.application_id):
        raise BuildError("ASTRA_ANDROID_APPLICATION_ID_INVALID")
    jdk_version, java = validate_java()
    sdk = sdk_root()
    require_tool("cargo")
    require_tool("cargo-ndk")

    abis = ["arm64-v8a"] + (["x86_64"] if args.with_emulator_abi else [])
    features = ["interpreter-only"]
    if any("jit" in feature.lower() for feature in features):
        raise BuildError("ASTRA_ANDROID_JIT_FORBIDDEN")

    executable_suffix = ".exe" if os.name == "nt" else ""
    toolchain = [
        artifact("jdk_runtime", java),
        artifact(
            "android_build_tools",
            sdk / f"build-tools/{BUILD_TOOLS}/aapt2{executable_suffix}",
        ),
        artifact(
            "android_ndk",
            ndk_prebuilt(sdk) / f"bin/clang{executable_suffix}",
        ),
        artifact(
            "gradle_wrapper",
            ANDROID_PROJECT / "gradle/wrapper/gradle-wrapper.jar",
        ),
    ]

    output = args.output.resolve()
    target_dir = output / "cargo-target"
    jni_dir = output / "jniLibs"
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    cargo_targets = ["arm64-v8a"] + (["x86_64"] if args.with_emulator_abi else [])
    run(
        ["cargo", "ndk", *sum((["-t", abi] for abi in cargo_targets), []), "-o", str(jni_dir), "build", "-p", "astra-player-android", "--release"],
        env=env,
    )
    copy_cxx_runtime(sdk, jni_dir, abis)

    gradle = ANDROID_PROJECT / ("gradlew.bat" if os.name == "nt" else "gradlew")
    gradle_args = [
        str(gradle),
        "--no-daemon",
        "assembleDebug",
        "assembleRelease",
        "bundleRelease",
        f"-PastraRustJniDir={jni_dir}",
        f"-PastraBundledPackage={package}",
        f"-PastraApplicationId={args.application_id}",
    ]
    signing_mode = "unsigned"
    if args.signing_properties:
        signing = args.signing_properties.resolve()
        if not signing.is_file():
            raise BuildError("ASTRA_ANDROID_SIGNING_PROPERTIES_MISSING")
        gradle_args.append(f"-PastraSigningProperties={signing}")
        signing_mode = "external"
    run(gradle_args, cwd=ANDROID_PROJECT, env=env)

    apk_root = ANDROID_PROJECT / "app/build/outputs/apk"
    bundle_root = ANDROID_PROJECT / "app/build/outputs/bundle/release"
    gradle_paths = [
        ("apk", apk_root / "debug/app-debug.apk"),
        ("apk", apk_root / "release/app-release-unsigned.apk"),
        ("aab", bundle_root / "app-release.aab"),
    ]
    missing = [
        str(path.relative_to(ANDROID_PROJECT))
        for _, path in gradle_paths
        if not path.is_file()
    ]
    if missing:
        raise BuildError(f"ASTRA_ANDROID_ARTIFACT_MISSING: {','.join(missing)}")
    artifacts_dir = output / "artifacts"
    artifacts_dir.mkdir(parents=True, exist_ok=True)
    paths = []
    for kind, source in gradle_paths:
        destination = artifacts_dir / source.name
        shutil.copy2(source, destination)
        if sha256(source) != sha256(destination):
            raise BuildError("ASTRA_ANDROID_ARTIFACT_COPY_MISMATCH")
        paths.append((kind, destination))

    native = jni_dir / "arm64-v8a/libastra_player_android.so"
    manifest = {
        "schema": SCHEMA,
        "target": args.target,
        "profile": args.profile,
        "package_id": args.application_id,
        "package_hash": sha256(package),
        "build_fingerprint": build_fingerprint(
            package, abis, features, jdk_version, toolchain
        ),
        "min_sdk": 28,
        "compile_sdk": SDK_API,
        "target_sdk": SDK_API,
        "build_tools": BUILD_TOOLS,
        "ndk_version": NDK_VERSION,
        "agp_version": AGP_VERSION,
        "gradle_version": GRADLE_VERSION,
        "jdk_major": 17,
        "jdk_version": jdk_version,
        "toolchain": toolchain,
        "shipping_abis": ["arm64-v8a"],
        "test_abis": ["arm64-v8a", "x86_64"],
        "native_library": artifact("cdylib", native),
        "artifacts": [artifact(kind, path) for kind, path in paths],
        "signing_mode": signing_mode,
        "cargo_features": features,
    }
    output.mkdir(parents=True, exist_ok=True)
    (output / "android-bundle-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(manifest, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except BuildError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
