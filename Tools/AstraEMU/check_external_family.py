"""Exercise an independent public Family implementation with the existing Host."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def run(host: Path) -> None:
    from PIL import Image

    repo = Path(__file__).resolve().parents[2]
    private = repo / ".tmp"
    private.mkdir(exist_ok=True)
    # This project owns its target; never borrow another workspace's artifacts.
    with tempfile.TemporaryDirectory(prefix="external-family-", dir=private) as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        shutil.copyfile(Path(__file__).parent / "tests/fixtures/external_family.rs", project / "src/lib.rs")
        api = (repo / "Emulator/Source/FamilyApi/astra-emu-family-api").as_posix()
        manifest = f'''[package]
name = "external-family-probe"
version = "0.1.0"
edition = "2024"
[workspace]
[lib]
crate-type = ["cdylib"]
[dependencies]
abi_stable = "0.11.3"
astra-emu-family-api = {{ path = {json.dumps(api)}, features = ["diagnostic-bridge"] }}
tracing = "0.1"
'''
        (project / "Cargo.toml").write_text(manifest, encoding="utf-8")
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(project / "target")
        env["RUST_LOG"] = "info"
        for command in [
            ["cargo", "fmt", "--check"],
            ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
            ["cargo", "build"],
        ]:
            subprocess.run(command, cwd=project, env=env, check=True, timeout=600)
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1"], cwd=project, env=env, timeout=60
        ))
        astra = {p["name"] for p in metadata["packages"] if p["name"].startswith("astra-")}
        if astra != {"astra-emu-family-api"}:
            raise RuntimeError(f"unexpected engine dependencies: {sorted(astra)}")
        library = "external_family_probe.dll" if os.name == "nt" else (
            "libexternal_family_probe.dylib" if os.sys.platform == "darwin" else "libexternal_family_probe.so"
        )
        game = project / "game"
        game.mkdir()
        (game / "public.fixture").write_text("public development fixture\n", encoding="utf-8")
        base = {
            "plugin": str(project / "target/debug" / library), "game": str(game),
            "configuration": [{"id": "intensity", "value": {"Integer": 96}}],
            "frames": 12, "capture_frames": [0, 4],
            "inputs": [
                {"frame": 2, "event": {"type": "key", "code": "enter", "pressed": True}},
                {"frame": 3, "event": {"type": "key", "code": "enter", "pressed": False}},
            ],
        }
        cases = [
            ("valid", True, {}), ("cold-reopen", True, {}),
            ("bad-config", False, {"configuration": [{"id": "intensity", "value": {"Integer": 999}}]}),
            ("missing-input", False, {"inputs": []}),
        ]
        for name, valid, changes in cases:
            config = base | changes | {"output": str(project / f"{name}.png")}
            source = project / f"{name}.json"
            source.write_text(json.dumps(config), encoding="utf-8")
            result = subprocess.run([str(host), "--headless", str(source)], env=env, timeout=30)
            if (result.returncode == 0) != valid:
                raise RuntimeError(f"{name}: unexpected Host exit status {result.returncode}")
            if valid:
                for frame, expected in [(0, (96, 0, 0, 255)), (4, (0, 96, 0, 255))]:
                    with Image.open(project / f"{name}.png.frame-{frame}.png") as image:
                        rgba = image.convert("RGBA")
                        if rgba.size != (2, 2) or set(rgba.getdata()) != {expected}:
                            raise RuntimeError(f"{name}: frame {frame} pixels differ")
            print(f"{name}: passed", flush=True)
    print("External Family CPU-frame/NullAudio development regression passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True, type=Path, help="Manager built in the current exclusive workspace")
    args = parser.parse_args()
    host = args.host.resolve(strict=True)
    run(host)
