#!/usr/bin/env python3
"""Run model-assisted listening review for every public flagship audio asset."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

from common import Diagnostics, ToolFailure, load_json, sha256_file, write_json_atomic


API_KEY_ENV = "OPENROUTER_API_KEY"
DEFAULT_MODEL = "xiaomi/mimo-v2.5"
DEFAULT_SEED = 20_260_715
def _parse_content(content: Any) -> dict[str, Any]:
    if not isinstance(content, str):
        raise ToolFailure("NATIVEVN_OPENROUTER_RESPONSE_INVALID", "OpenRouter returned no textual structured review")
    stripped = content.strip()
    if stripped.startswith("```"):
        lines = stripped.splitlines()
        if len(lines) >= 3 and lines[-1].strip() == "```":
            stripped = "\n".join(lines[1:-1])
    try:
        payload = json.loads(stripped)
    except json.JSONDecodeError as error:
        raise ToolFailure("NATIVEVN_OPENROUTER_RESPONSE_INVALID", "OpenRouter review was not valid JSON") from error
    if not isinstance(payload, dict):
        raise ToolFailure("NATIVEVN_OPENROUTER_RESPONSE_INVALID", "OpenRouter review did not match the required response contract")
    normalizations: list[str] = []
    decision = payload.get("decision")
    if decision is None:
        for alias in ("verdict", "status", "result"):
            if isinstance(payload.get(alias), str):
                decision = payload[alias]
                normalizations.append(f"{alias}_to_decision")
                break
    decision_map = {"pass": "pass", "passed": "pass", "accept": "pass", "accepted": "pass", "approve": "pass", "approved": "pass", "ok": "pass", "blocked": "blocked", "fail": "blocked", "failed": "blocked", "reject": "blocked", "rejected": "blocked"}
    normalized_decision = decision_map.get(str(decision).strip().lower())
    summary = payload.get("summary")
    if summary is None:
        for alias in ("notes", "reason", "assessment", "comments"):
            if isinstance(payload.get(alias), str):
                summary = payload[alias]
                normalizations.append(f"{alias}_to_summary")
                break
    defects = payload.get("defects")
    if defects is None and isinstance(payload.get("issues"), list):
        defects = payload["issues"]
        normalizations.append("issues_to_defects")
    if defects is None and normalized_decision == "pass":
        defects = []
        normalizations.append("empty_defects_from_pass")
    if defects is None and normalized_decision == "blocked":
        defects = ["model_reported_blocker"]
        normalizations.append("blocker_code_from_decision")
    fit_for_role = payload.get("fit_for_role")
    if fit_for_role is None and normalized_decision is not None:
        fit_for_role = normalized_decision == "pass"
        normalizations.append("fit_from_decision")
    if (
        normalized_decision is None
        or not isinstance(defects, list)
        or not all(isinstance(item, str) for item in defects)
        or not isinstance(summary, str)
        or not isinstance(fit_for_role, bool)
    ):
        raise ToolFailure("NATIVEVN_OPENROUTER_RESPONSE_INVALID", "OpenRouter review did not match the required response contract")
    return {"decision": normalized_decision, "defects": defects, "summary": summary, "fit_for_role": fit_for_role, "contract_normalizations": normalizations}


def _transcode_mp3(source: Path, destination: Path) -> None:
    completed = subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i", str(source), "-map_metadata", "-1", "-vn", "-c:a", "libmp3lame", "-b:a", "128k", str(destination)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0 or not destination.is_file() or destination.stat().st_size == 0:
        raise ToolFailure("NATIVEVN_AUDIO_REVIEW_TRANSCODE_FAILED", "ffmpeg could not create a temporary MP3 review input", path=source.name)


def _request(api_key: str, model: str, seed: int, audio_path: Path, prompt: str, timeout: float) -> tuple[dict[str, Any], dict[str, Any]]:
    body = {
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "input_audio", "input_audio": {"data": base64.b64encode(audio_path.read_bytes()).decode("ascii"), "format": "mp3"}},
            ],
        }],
        "temperature": 0,
        "seed": seed,
        "max_tokens": 4_000,
        "response_format": {"type": "json_object"},
    }
    request = urllib.request.Request(
        "https://openrouter.ai/api/v1/chat/completions",
        data=json.dumps(body, ensure_ascii=False).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
            "HTTP-Referer": "https://github.com/AstraEngine",
            "X-Title": "AstraEngine NativeVN Audio QA",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            envelope = json.load(response)
    except urllib.error.HTTPError as error:
        raise ToolFailure("NATIVEVN_OPENROUTER_REQUEST_REJECTED", f"OpenRouter rejected an audio review with HTTP {error.code}") from error
    except (urllib.error.URLError, TimeoutError) as error:
        raise ToolFailure("NATIVEVN_OPENROUTER_UNAVAILABLE", "OpenRouter audio review request failed") from error
    try:
        review = _parse_content(envelope["choices"][0]["message"]["content"])
    except (KeyError, IndexError, TypeError) as error:
        raise ToolFailure("NATIVEVN_OPENROUTER_RESPONSE_INVALID", "OpenRouter response envelope was incomplete") from error
    metadata = {
        "model": envelope.get("model"),
        "provider": envelope.get("provider"),
        "usage": envelope.get("usage", {}),
    }
    return review, metadata


def review(audio_root: Path, *, model: str = DEFAULT_MODEL, seed: int = DEFAULT_SEED, timeout: float = 180.0) -> dict[str, Any]:
    api_key = os.environ.get(API_KEY_ENV, "").strip()
    if not api_key:
        raise ToolFailure("NATIVEVN_OPENROUTER_API_KEY_MISSING", f"{API_KEY_ENV} is required; no model review was written")
    manifest = load_json(audio_root / "audio-manifest.json")
    automated = load_json(audio_root / "audio-qa-report.json")
    automated_by_id = {item["id"]: item for item in automated.get("assets", []) if isinstance(item, dict) and isinstance(item.get("id"), str)}
    checkpoint_path = audio_root.parent / ".local" / "review" / "openrouter-audio-review.partial.json"
    results: list[dict[str, Any]] = []
    if checkpoint_path.is_file():
        checkpoint = load_json(checkpoint_path)
        if checkpoint.get("model") == model and checkpoint.get("seed") == seed and isinstance(checkpoint.get("results"), list):
            results = checkpoint["results"]
    completed_ids = {item.get("id") for item in results if isinstance(item, dict)}
    with tempfile.TemporaryDirectory(prefix="nativevn-audio-review-") as temporary:
        temporary_root = Path(temporary)
        for index, asset in enumerate(manifest.get("assets", []), start=1):
            if asset["id"] in completed_ids:
                continue
            source = audio_root / asset["distribution"]["path"]
            review_input = temporary_root / f"{asset['id']}.mp3"
            _transcode_mp3(source, review_input)
            metrics = automated_by_id.get(asset["id"], {})
            prompt = (
                "You are the independent listening reviewer for a restrained 12+ science-fiction visual novel. "
                f"This asset is id={asset['id']}, kind={asset['kind']}, title={asset['title']['en']}, loop={str(asset['loop']).lower()}. "
                "Listen to the entire attached file. Check unintended silence, clipping, clicks, discontinuities, harsh digital artifacts, broken attack or tail, excessive noise, stereo imbalance, and whether it fits the declared role. "
                "For a looping BGM, judge whether the ending can reconnect cleanly to the beginning. Intentional rain, carrier noise, glitches, impacts, and short silence-shaped envelopes are not defects by themselves. "
                f"Automated measurements: integrated_loudness_lufs={metrics.get('integrated_loudness_lufs')}, true_peak_dbtp={metrics.get('true_peak_dbtp')}, loop_seam_max_delta={metrics.get('loop_seam_max_delta')}, clipped_samples={metrics.get('clipped_samples')}. "
                "Block only for a concrete audible defect or role mismatch. Return the required JSON without markdown."
            )
            print(json.dumps({"event": "openrouter.audio_review.started", "asset_id": asset["id"], "index": index, "total": len(manifest["assets"])}, sort_keys=True), file=sys.stderr)
            attempt_count = 0
            while True:
                attempt_count += 1
                try:
                    decision, metadata = _request(api_key, model, seed + index + (attempt_count - 1) * 100_000, review_input, prompt, timeout)
                    break
                except ToolFailure as error:
                    if error.code not in {"NATIVEVN_OPENROUTER_RESPONSE_INVALID", "NATIVEVN_OPENROUTER_UNAVAILABLE"} or attempt_count >= 4:
                        raise
                    print(json.dumps({"event": "openrouter.audio_review.retry", "asset_id": asset["id"], "attempt": attempt_count + 1, "reason": error.code}, sort_keys=True), file=sys.stderr)
            response_hash = hashlib.sha256(json.dumps(decision, ensure_ascii=False, sort_keys=True).encode("utf-8")).hexdigest()
            results.append({
                "id": asset["id"],
                "kind": asset["kind"],
                "source_sha256": sha256_file(source),
                "decision": decision["decision"],
                "fit_for_role": decision["fit_for_role"],
                "defects": decision["defects"],
                "summary": decision["summary"],
                "contract_normalizations": decision["contract_normalizations"],
                "response_sha256": response_hash,
                "selected_model": metadata["model"],
                "selected_provider": metadata["provider"],
                "usage": metadata["usage"],
                "attempt_count": attempt_count,
            })
            checkpoint_path.parent.mkdir(parents=True, exist_ok=True)
            write_json_atomic(checkpoint_path, {"schema": "astra.nativevn_flagship.openrouter_audio_review_checkpoint.v1", "id": "nativevn-flagship-openrouter-audio-review-checkpoint", "model": model, "seed": seed, "results": results})
    payload = {
        "schema": "astra.nativevn_flagship.openrouter_audio_review.v1",
        "id": "nativevn-flagship-openrouter-audio-review",
        "selection": {
            "required_input_modality": "audio",
            "strategy": "capability_and_region_probe",
            "selected_model": model,
            "rationale": "OpenRouter audio-capable structured model verified in the active region; auto and higher-tier audio endpoints failed capability or regional preflight.",
        },
        "request": {
            "requested_model": model,
            "temperature": 0,
            "seed_base": seed,
            "reasoning_effort": "disabled_for_bounded_qa",
            "response_format": "json_object",
            "input_format": "mp3_128k_temporary",
        },
        "asset_count": len(results),
        "decision": "pass" if results and all(item["decision"] == "pass" and item["fit_for_role"] for item in results) else "blocked",
        "human_review_replaced": False,
        "assets": results,
    }
    write_json_atomic(audio_root / "openrouter-audio-review.json", payload)
    checkpoint_path.unlink(missing_ok=True)
    return payload


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audio-root", type=Path, default=Path(__file__).resolve().parents[2] / "Examples" / "NativeVN" / "Audio")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--timeout", type=float, default=180.0)
    args = parser.parse_args(argv)
    diagnostics = Diagnostics()
    try:
        report = review(args.audio_root.resolve(), model=args.model, seed=args.seed, timeout=args.timeout)
    except (OSError, ValueError, KeyError, json.JSONDecodeError, ToolFailure) as error:
        if isinstance(error, ToolFailure):
            diagnostics.error(error.code, error.message, error.path)
        else:
            diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_FAILED", "model-assisted audio review failed while processing an asset")
        diagnostics.emit_json()
        return 2
    if report["decision"] != "pass":
        diagnostics.error("NATIVEVN_OPENROUTER_REVIEW_BLOCKED", "one or more audio assets failed model-assisted listening review")
        diagnostics.emit_json(summary={"asset_count": report["asset_count"]})
        return 2
    diagnostics.emit_json(summary={"asset_count": report["asset_count"], "model": args.model, "human_review_replaced": False})
    return 0


if __name__ == "__main__":
    sys.exit(main())
