#!/usr/bin/env python3
"""Build and attest the bounded Web UI component fixture with pinned jco."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys


def digest(path: pathlib.Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def run(command: list[str], root: pathlib.Path) -> None:
    completed = subprocess.run(
        command,
        cwd=root,
        text=True,
        encoding="utf-8",
        errors="strict",
        capture_output=True,
    )
    if completed.returncode:
        raise RuntimeError(
            "ASTRA_UI_WEB_COMPONENT_BUILD_FAILED: " + completed.stderr.strip()
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=pathlib.Path)
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parents[1]
    preflight = root / ".tmp" / "ui-toolchain" / "preflight.json"
    run(
        [
            sys.executable,
            str(root / "Tools" / "bootstrap_ui_toolchain.py"),
            "--output",
            str(preflight),
        ],
        root,
    )
    toolchain = json.loads(preflight.read_text(encoding="utf-8"))
    if toolchain.get("schema") != "astra.ui_toolchain_preflight.v1":
        raise RuntimeError("ASTRA_UI_WEB_COMPONENT_PREFLIGHT_INVALID")
    output = (args.output or root / ".tmp" / "ui-component-web").resolve()
    allowed = (root / ".tmp").resolve()
    if allowed not in output.parents:
        raise RuntimeError("ASTRA_UI_WEB_COMPONENT_OUTPUT_OUTSIDE_PRIVATE_ROOT")
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    wit = root / "Engine" / "Source" / "Runtime" / "astra-ui-plugin-abi" / "wit" / "ui-component.wit"
    source = root / "Engine" / "Plugins" / "Fixtures" / "ui-component-web" / "component.js"
    jco = root / "Tools" / "ui-component-web" / "node_modules" / ".bin" / (
        "jco.cmd" if sys.platform == "win32" else "jco"
    )
    component = output / "astra-ui-component.fixture.wasm"
    es_module = output / "es"
    run(
        [str(jco), "componentize", str(source), "--wit", str(wit), "--world-name", "ui-component", "--disable", "all", "-o", str(component)],
        root,
    )
    run(
        [str(jco), "transpile", str(component), "--out-dir", str(es_module), "--name", "astra-ui-component.fixture", "--no-nodejs-compat", "--strict", "--quiet", "--instantiation", "async"],
        root,
    )
    shutil.copyfile(
        root / "Engine" / "Source" / "Programs" / "astra-player-web" / "web" / "astra-ui-component-host.js",
        output / "astra-ui-component-host.js",
    )
    (output / "package.json").write_text('{"private":true,"type":"module"}\n', encoding="utf-8")
    artifacts = []
    for path in sorted(item for item in output.rglob("*") if item.is_file()):
        artifacts.append(
            {
                "path": path.relative_to(output).as_posix(),
                "sha256": digest(path),
                "byte_size": path.stat().st_size,
            }
        )
    wasm_artifacts = [artifact for artifact in artifacts if str(artifact["path"]).endswith(".wasm")]
    js_artifacts = [artifact for artifact in artifacts if str(artifact["path"]).endswith(".js")]
    if len(wasm_artifacts) < 2 or not js_artifacts:
        raise RuntimeError("ASTRA_UI_WEB_COMPONENT_OUTPUT_INCOMPLETE")
    core = next(artifact for artifact in wasm_artifacts if str(artifact["path"]).endswith(".core.wasm"))
    smoke = output / "smoke.mjs"
    smoke.write_text(
        "import { createUiComponentSession } from './astra-ui-component-host.js';\n"
        "const bindingsUrl = new URL('./es/astra-ui-component.fixture.js', import.meta.url);\n"
        f"const coreArtifacts = new Map([['astra-ui-component.fixture.core.wasm', {{sha256:'{core['sha256']}',byteSize:{core['byte_size']}}}]]);\n"
        "const { readFile } = await import('node:fs/promises');\n"
        "const fetchArtifact = async (path) => { const bytes = await readFile(new URL('./es/' + path, import.meta.url)); return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength); };\n"
        "const session = await createUiComponentSession({bindingsUrl,coreArtifacts,fetchArtifact});\n"
        "const hash = new Uint8Array(32); const payload = new Uint8Array([1,2,3]);\n"
        "const result = session.invoke(1, 7n, 1n, hash, payload);\n"
        "if (result[1] !== 7n || result[3].length !== 3) throw new Error('roundtrip');\n"
        "let rejected = false; try { session.invoke(1, 8n, 1n, hash, new Uint8Array(4194305)); } catch { rejected = true; }\n"
        "if (!rejected) throw new Error('limit');\n",
        encoding="utf-8",
    )
    run([str(pathlib.Path(shutil.which("node") or "node")), str(smoke)], output)
    smoke.unlink()
    bundle_files = []
    for path in sorted(item for item in es_module.rglob("*") if item.is_file()):
        relative = path.relative_to(es_module).as_posix()
        data = path.read_bytes()
        bundle_files.append(
            {
                "path": relative,
                "sha256": digest(path),
                "byte_size": len(data),
                "base64": base64.b64encode(data).decode("ascii"),
            }
        )
    bindings = next(
        item["path"]
        for item in bundle_files
        if item["path"].endswith(".js") and not item["path"].endswith(".d.js")
    )
    web_artifact = {
        "schema": "astra.ui_component_web_artifact.v1",
        "bindings": bindings,
        "files": bundle_files,
    }
    web_artifact_path = output / "astra-ui-component-web-artifact.json"
    web_artifact_path.write_text(
        json.dumps(web_artifact, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    report: dict[str, object] = {
        "schema": "astra.ui_component_web_bundle.v1",
        "input": {
            "source_sha256": digest(source),
            "wit_sha256": digest(wit),
            "toolchain_lock_sha256": toolchain["lock_sha256"],
            "jco_version": toolchain["tools"]["jco"]["version"],
            "jco_package_sha256": toolchain["tools"]["jco"]["package_sha256"],
        },
        "limits": {"dto_bytes": 4 * 1024 * 1024, "memory_bytes": 64 * 1024 * 1024},
        "artifacts": artifacts,
        "signed_artifact": {
            "path": web_artifact_path.name,
            "sha256": digest(web_artifact_path),
            "byte_size": web_artifact_path.stat().st_size,
        },
    }
    canonical = json.dumps(report, sort_keys=True, separators=(",", ":")).encode()
    report["bundle_hash"] = "sha256:" + hashlib.sha256(canonical).hexdigest()
    report_path = output / "astra-ui-component-web-report.json"
    report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(report, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
