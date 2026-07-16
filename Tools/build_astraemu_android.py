#!/usr/bin/env python3
"""Build and verify the signed AstraEMU Android package.

The command deliberately fails before compilation when the SDK, NDK, trust
roots, or signing identity are incomplete. It never writes secret values to a
report or command line other than the family signer key's pre-existing env var.
"""

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
import tempfile
import urllib.request
import zipfile


GRADLE_VERSION = "8.13"
GRADLE_SHA256 = "20f1b1176237254a6fc204d8434196fa11a4cfb387567519c61556e8710aed78"
GRADLE_URL = f"https://services.gradle.org/distributions/gradle-{GRADLE_VERSION}-bin.zip"
PACKAGE_NAME = "org.astraemu.manager"
MIN_API = 26
TARGET_API = 36
ABI_TARGETS = {
    "arm64-v8a": "aarch64-linux-android",
    "x86_64": "x86_64-linux-android",
}
REQUIRED_SECRET_ENV = (
    "ASTRA_EMU_FAMILY_SIGNING_KEY_HEX",
    "ASTRA_EMU_FAMILY_SIGNER_ID",
    "ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX",
    "ASTRA_EMU_ANDROID_KEYSTORE",
    "ASTRA_EMU_ANDROID_KEYSTORE_PASSWORD",
    "ASTRA_EMU_ANDROID_KEY_ALIAS",
    "ASTRA_EMU_ANDROID_KEY_PASSWORD",
    "ASTRA_EMU_ANDROID_APK_SIGNER_SHA256",
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--abi", action="append", choices=sorted(ABI_TARGETS))
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--version-code", type=int, default=1)
    parser.add_argument("--version-name", default="0.1.0")
    args = parser.parse_args()

    root = pathlib.Path(__file__).resolve().parents[1]
    platform = root / "AstraEMU" / "Platforms" / "Android"
    output = (args.output or root / ".tmp" / "android-package").resolve()
    ensure_descendant(output, root)
    abis = args.abi or list(ABI_TARGETS)
    if len(set(abis)) != len(abis):
        fail("ASTRA_EMU_ANDROID_ABI_DUPLICATE")
    if args.version_code <= 0 or args.version_code > 2_100_000_000:
        fail("ASTRA_EMU_ANDROID_VERSION_CODE")
    if not re.fullmatch(r"[0-9A-Za-z][0-9A-Za-z._+-]{0,63}", args.version_name):
        fail("ASTRA_EMU_ANDROID_VERSION_NAME")
    require_environment()

    sdk = locate_sdk()
    ndk = locate_ndk(sdk)
    toolchain = locate_ndk_toolchain(ndk)
    apksigner = locate_build_tool(sdk, "apksigner")
    require_sdk_api(sdk, TARGET_API)
    require_rust_targets(abis)
    gradle = provision_gradle(root)

    generated = platform / "generated"
    reset_generated(generated, platform)
    target_root = output / "cargo-target"
    target_root.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_root)
    env["ASTRA_EMU_ANDROID_VERSION_CODE"] = str(args.version_code)
    env["ASTRA_EMU_ANDROID_VERSION_NAME"] = args.version_name

    manifests: dict[str, tuple[pathlib.Path, pathlib.Path]] = {}
    for abi in abis:
        target = ABI_TARGETS[abi]
        target_env = configure_target_environment(env, toolchain, target)
        descriptor = cargo_build_android(root, target, target_root, target_env)
        target_dir = target_root / target / "release"
        manager = target_dir / "libastra_emu_manager.so"
        family = target_dir / "libastra_emu_fvp.so"
        for library in (manager, family):
            require_file(library, "ASTRA_EMU_ANDROID_LIBRARY_MISSING")
            verify_elf_alignment(toolchain, library)
        jni_dir = generated / "jniLibs" / abi
        jni_dir.mkdir(parents=True, exist_ok=True)
        shutil.copy2(manager, jni_dir / manager.name)
        shutil.copy2(family, jni_dir / family.name)

        metadata_dir = output / "metadata" / abi
        if metadata_dir.exists():
            shutil.rmtree(metadata_dir)
        run(
            [
                "cargo",
                "run",
                "--quiet",
                "-p",
                "astra-emu-family-package",
                "--",
                "android-sign",
                "--binary",
                str(family),
                "--descriptor",
                str(descriptor),
                "--output-dir",
                str(metadata_dir),
                "--package-name",
                PACKAGE_NAME,
                "--version-code",
                str(args.version_code),
                "--abi",
                abi,
                "--apk-signer-sha256",
                env["ASTRA_EMU_ANDROID_APK_SIGNER_SHA256"],
                "--signer-identity",
                env["ASTRA_EMU_FAMILY_SIGNER_ID"],
                "--min-api",
                str(MIN_API),
                "--target-api",
                str(TARGET_API),
            ],
            root,
            env,
        )
        asset_dir = generated / "assets" / "astraemu" / "families" / "fvp" / abi
        asset_dir.mkdir(parents=True, exist_ok=True)
        family_manifest = metadata_dir / "manifest.json"
        native_manifest = metadata_dir / "native-manifest.json"
        shutil.copy2(family_manifest, asset_dir / family_manifest.name)
        shutil.copy2(native_manifest, asset_dir / native_manifest.name)
        manifests[abi] = (family_manifest, native_manifest)

    run([str(gradle), "--no-daemon", "--stacktrace", ":app:assembleRelease"], platform, env)
    built_apk = platform / "app" / "build" / "outputs" / "apk" / "release" / "app-release.apk"
    require_file(built_apk, "ASTRA_EMU_ANDROID_APK_MISSING")
    verify_apk(apksigner, built_apk, abis, env["ASTRA_EMU_ANDROID_APK_SIGNER_SHA256"])
    output.mkdir(parents=True, exist_ok=True)
    final_apk = output / "astraemu-release.apk"
    temporary_apk = final_apk.with_suffix(".apk.partial")
    shutil.copy2(built_apk, temporary_apk)
    temporary_apk.replace(final_apk)
    write_report(output, final_apk, abis, manifests, args.version_code)
    print(json.dumps({"status": "PASS", "artifact": final_apk.name}, sort_keys=True))
    return 0


def require_environment() -> None:
    missing = [name for name in REQUIRED_SECRET_ENV if not os.environ.get(name)]
    if missing:
        fail("ASTRA_EMU_ANDROID_SIGNING_ENV_MISSING:" + ",".join(missing))
    signer = os.environ["ASTRA_EMU_ANDROID_APK_SIGNER_SHA256"]
    if not re.fullmatch(r"sha256\.[0-9a-f]{64}", signer):
        fail("ASTRA_EMU_ANDROID_APK_SIGNER_HASH")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", os.environ["ASTRA_EMU_FAMILY_PUBLIC_KEY_HEX"]):
        fail("ASTRA_EMU_FAMILY_PUBLIC_KEY_ENCODING")


def locate_sdk() -> pathlib.Path:
    value = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
    if not value:
        fail("ASTRA_EMU_ANDROID_SDK_MISSING")
    sdk = pathlib.Path(value).resolve()
    if not sdk.is_dir():
        fail("ASTRA_EMU_ANDROID_SDK_MISSING")
    return sdk


def locate_ndk(sdk: pathlib.Path) -> pathlib.Path:
    explicit = os.environ.get("ANDROID_NDK_HOME")
    candidates = [pathlib.Path(explicit)] if explicit else list((sdk / "ndk").glob("*"))
    candidates = [path.resolve() for path in candidates if path.is_dir()]
    if not candidates:
        fail("ASTRA_EMU_ANDROID_NDK_MISSING")
    candidates.sort(key=lambda path: version_tuple(path.name), reverse=True)
    ndk = candidates[0]
    if version_tuple(ndk.name)[0] < 28:
        fail("ASTRA_EMU_ANDROID_NDK_16K_UNSUPPORTED")
    return ndk


def locate_ndk_toolchain(ndk: pathlib.Path) -> pathlib.Path:
    prebuilt = ndk / "toolchains" / "llvm" / "prebuilt"
    hosts = [path for path in prebuilt.iterdir() if path.is_dir()]
    if len(hosts) != 1:
        fail("ASTRA_EMU_ANDROID_NDK_HOST_TOOLCHAIN")
    return hosts[0]


def locate_build_tool(sdk: pathlib.Path, name: str) -> pathlib.Path:
    suffix = ".bat" if os.name == "nt" else ""
    candidates = sorted((sdk / "build-tools").glob(f"*/{name}{suffix}"), reverse=True)
    if not candidates:
        fail("ASTRA_EMU_ANDROID_BUILD_TOOL_MISSING")
    return candidates[0]


def require_sdk_api(sdk: pathlib.Path, api: int) -> None:
    require_file(sdk / "platforms" / f"android-{api}" / "android.jar", "ASTRA_EMU_ANDROID_API36_MISSING")


def require_rust_targets(abis: list[str]) -> None:
    installed = set(run_capture(["rustup", "target", "list", "--installed"], None).splitlines())
    missing = [ABI_TARGETS[abi] for abi in abis if ABI_TARGETS[abi] not in installed]
    if missing:
        fail("ASTRA_EMU_ANDROID_RUST_TARGET_MISSING:" + ",".join(missing))


def provision_gradle(root: pathlib.Path) -> pathlib.Path:
    cache = root / ".tmp" / "toolchains"
    archive = cache / f"gradle-{GRADLE_VERSION}-bin.zip"
    home = cache / f"gradle-{GRADLE_VERSION}"
    executable = home / "bin" / ("gradle.bat" if os.name == "nt" else "gradle")
    if executable.is_file():
        return executable
    cache.mkdir(parents=True, exist_ok=True)
    if not archive.is_file() or sha256_file(archive) != GRADLE_SHA256:
        archive.unlink(missing_ok=True)
        partial = archive.with_suffix(".zip.partial")
        with urllib.request.urlopen(GRADLE_URL, timeout=60) as response, partial.open("wb") as output:
            shutil.copyfileobj(response, output)
        if sha256_file(partial) != GRADLE_SHA256:
            partial.unlink(missing_ok=True)
            fail("ASTRA_EMU_GRADLE_DISTRIBUTION_HASH")
        partial.replace(archive)
    with zipfile.ZipFile(archive) as bundle:
        for member in bundle.infolist():
            target = (cache / member.filename).resolve()
            ensure_descendant(target, cache)
        bundle.extractall(cache)
    require_file(executable, "ASTRA_EMU_GRADLE_DISTRIBUTION_INVALID")
    return executable


def configure_target_environment(
    base: dict[str, str], toolchain: pathlib.Path, target: str
) -> dict[str, str]:
    env = base.copy()
    bin_dir = toolchain / "bin"
    compiler = bin_dir / f"{target}{MIN_API}-clang"
    if os.name == "nt":
        compiler = compiler.with_suffix(".cmd")
    ar = bin_dir / ("llvm-ar.exe" if os.name == "nt" else "llvm-ar")
    require_file(compiler, "ASTRA_EMU_ANDROID_NDK_CLANG_MISSING")
    require_file(ar, "ASTRA_EMU_ANDROID_NDK_AR_MISSING")
    key = target.upper().replace("-", "_")
    env[f"CARGO_TARGET_{key}_LINKER"] = str(compiler)
    env[f"CC_{target.replace('-', '_')}"] = str(compiler)
    env[f"AR_{target.replace('-', '_')}"] = str(ar)
    return env


def cargo_build_android(
    root: pathlib.Path,
    target: str,
    target_root: pathlib.Path,
    env: dict[str, str],
) -> pathlib.Path:
    command = [
        "cargo",
        "build",
        "--release",
        "--target",
        target,
        "-p",
        "astra-emu-fvp",
        "-p",
        "astra-emu-manager",
        "--lib",
        "--message-format=json-render-diagnostics",
    ]
    process = subprocess.Popen(
        command,
        cwd=root,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=None,
    )
    descriptor: pathlib.Path | None = None
    assert process.stdout is not None
    for line in process.stdout:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") == "build-script-executed" and "astra-emu-fvp" in message.get("package_id", ""):
            candidate = pathlib.Path(message["out_dir"]) / "astra-fvp-descriptor.json"
            descriptor = candidate
    if process.wait() != 0:
        fail("ASTRA_EMU_ANDROID_CARGO_BUILD_FAILED")
    if descriptor is None:
        candidates = list(target_root.glob(f"{target}/release/build/astra-emu-fvp-*/out/astra-fvp-descriptor.json"))
        if len(candidates) != 1:
            fail("ASTRA_EMU_FVP_DESCRIPTOR_MISSING")
        descriptor = candidates[0]
    require_file(descriptor, "ASTRA_EMU_FVP_DESCRIPTOR_MISSING")
    return descriptor


def verify_elf_alignment(toolchain: pathlib.Path, library: pathlib.Path) -> None:
    readelf = toolchain / "bin" / ("llvm-readelf.exe" if os.name == "nt" else "llvm-readelf")
    require_file(readelf, "ASTRA_EMU_ANDROID_READELF_MISSING")
    output = run_capture([str(readelf), "--program-headers", "--wide", str(library)], None)
    load_lines = [line for line in output.splitlines() if line.lstrip().startswith("LOAD")]
    if not load_lines:
        fail("ASTRA_EMU_ANDROID_ELF_LOAD_SEGMENTS_MISSING")
    for line in load_lines:
        match = re.search(r"0x([0-9a-fA-F]+)\s*$", line)
        if not match or int(match.group(1), 16) < 0x4000:
            fail("ASTRA_EMU_ANDROID_ELF_16K_ALIGNMENT")


def verify_apk(
    apksigner: pathlib.Path, apk: pathlib.Path, abis: list[str], expected_signer: str
) -> None:
    output = run_capture([str(apksigner), "verify", "--verbose", "--print-certs", str(apk)], None)
    match = re.search(r"Signer #1 certificate SHA-256 digest:\s*([0-9a-fA-F]{64})", output)
    if not match or "sha256." + match.group(1).lower() != expected_signer:
        fail("ASTRA_EMU_ANDROID_APK_SIGNER_MISMATCH")
    with zipfile.ZipFile(apk) as bundle:
        names = set(bundle.namelist())
        for abi in abis:
            required = {
                f"lib/{abi}/libastra_emu_manager.so",
                f"lib/{abi}/libastra_emu_fvp.so",
                f"assets/astraemu/families/fvp/{abi}/manifest.json",
                f"assets/astraemu/families/fvp/{abi}/native-manifest.json",
            }
            if not required.issubset(names):
                fail("ASTRA_EMU_ANDROID_APK_CONTENT_MISSING")


def write_report(
    output: pathlib.Path,
    apk: pathlib.Path,
    abis: list[str],
    manifests: dict[str, tuple[pathlib.Path, pathlib.Path]],
    version_code: int,
) -> None:
    report = {
        "schema": "astra.emu.android_package_evidence.v1",
        "status": "PASS",
        "package_name": PACKAGE_NAME,
        "version_code": version_code,
        "min_api": MIN_API,
        "target_api": TARGET_API,
        "abis": abis,
        "apk_sha256": "sha256." + sha256_file(apk),
        "family_manifest_hashes": {
            abi: "sha256." + sha256_file(paths[0]) for abi, paths in manifests.items()
        },
        "native_manifest_hashes": {
            abi: "sha256." + sha256_file(paths[1]) for abi, paths in manifests.items()
        },
        "elf_page_alignment": 16384,
        "diagnostic_codes": [],
    }
    temporary = output / "android-package-evidence.json.partial"
    temporary.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(output / "android-package-evidence.json")


def reset_generated(path: pathlib.Path, platform: pathlib.Path) -> None:
    ensure_descendant(path.resolve(), platform.resolve())
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def ensure_descendant(path: pathlib.Path, parent: pathlib.Path) -> None:
    try:
        path.relative_to(parent.resolve())
    except ValueError:
        fail("ASTRA_EMU_PATH_OUTSIDE_WORKSPACE")


def require_file(path: pathlib.Path, code: str) -> None:
    if not path.is_file() or path.stat().st_size == 0:
        fail(code)


def version_tuple(value: str) -> tuple[int, ...]:
    numbers = [int(part) for part in re.findall(r"\d+", value)]
    return tuple(numbers or [0])


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(command: list[str], cwd: pathlib.Path, env: dict[str, str]) -> None:
    result = subprocess.run(command, cwd=cwd, env=env, check=False)
    if result.returncode != 0:
        fail("ASTRA_EMU_EXTERNAL_COMMAND_FAILED")


def run_capture(command: list[str], cwd: pathlib.Path | None) -> str:
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, check=False)
    if result.returncode != 0:
        fail("ASTRA_EMU_EXTERNAL_COMMAND_FAILED")
    return result.stdout + result.stderr


def fail(code: str) -> None:
    raise SystemExit(code)


if __name__ == "__main__":
    sys.exit(main())
