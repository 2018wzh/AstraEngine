from __future__ import annotations

import hashlib
import io
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import zlib
from collections import deque
from pathlib import Path

from tsuinosora_constants import *
from tsuinosora_diagnostics import _write_json
from tsuinosora_rendering import _read_json

__all__ = ['_run_director_story_source_from_demo_config', '_blocked_story_graph_report', '_blocked_scene_semantic_report', '_blocked_asset_binding_report', '_blocked_story_program_report']


def _run_director_story_source_from_demo_config(config: dict) -> dict | None:
    roots = config.get("projectorrays_full_dump_roots")
    if not roots:
        return None
    work_root = Path(str(config.get("local_work_root", "")))
    dump_roots = [
        (str(item.get("alias", "")), Path(str(item.get("path", ""))))
        for item in roots
        if isinstance(item, dict)
    ]
    if {alias for alias, _ in dump_roots} != {"root", "data", "casts"}:
        return None
    for stale in (
        work_root / "private" / "native_story_ir.json",
        work_root / "reports" / "director_native_story_lowering_report.json",
    ):
        try:
            stale.unlink()
        except FileNotFoundError:
            pass
    try:
        detailed, report = build_director_story_source(work_root, dump_roots)
    except DirectorStorySourceError as exc:
        report = {
            "schema": "tsuinosora.director_story_source_report.v1",
            "status": "blocked",
            "movie_count": 0,
            "frame_count": 0,
            "label_count": 0,
            "out_of_score_label_count": 0,
            "label_action_binding_count": 0,
            "frame_action_binding_count": 0,
            "scene_text_binding_count": 0,
            "named_text_member_count": 0,
            "script_resource_count": 0,
            "story_source_sha256": "sha256:" + "0" * 64,
            "movie_coverage": [],
            "diagnostics": [
                {
                    "code": "TSUI_DIRECTOR_STORY_SOURCE_BLOCKED",
                    "message": str(exc),
                }
            ],
            "redaction": {
                "paths": "alias_or_report_relative_only",
                "payload": "omitted",
                "commercial_text": "omitted",
                "script_source": "omitted",
            },
        }
    else:
        _write_json(work_root / "private" / "director_story_source.json", detailed)
        try:
            scene_dsl, scene_report = build_scene_dsl_ir(detailed)
        except DirectorSceneDslError as exc:
            scene_report = {
                "schema": "tsuinosora.director_scene_dsl_report.v1",
                "status": "blocked",
                "source_scene_count": report["scene_text_binding_count"],
                "converted_scene_count": 0,
                "source_line_count": 0,
                "operation_counts": {},
                "termination_counts": {},
                "scene_dsl_sha256": "sha256:" + "0" * 64,
                "diagnostics": [
                    {"code": "TSUI_DIRECTOR_SCENE_DSL_BLOCKED", "message": str(exc)}
                ],
                "redaction": {
                    "paths": "alias_or_report_relative_only",
                    "payload": "omitted",
                    "commercial_text": "private_ir_only",
                },
            }
            report["status"] = "blocked"
            report["diagnostics"].extend(scene_report["diagnostics"])
        else:
            _write_json(work_root / "private" / "director_scene_dsl.json", scene_dsl)
        _write_json(work_root / "reports" / "director_scene_dsl_report.json", scene_report)
        if scene_report["status"] == "pass":
            try:
                scene_semantics, semantic_report = build_scene_semantic_ir(scene_dsl)
            except DirectorSceneSemanticError as exc:
                semantic_report = _blocked_scene_semantic_report(str(exc))
                report["status"] = "blocked"
                report["diagnostics"].extend(semantic_report["diagnostics"])
            else:
                _write_json(work_root / "private" / "director_scene_semantics.json", scene_semantics)
        else:
            semantic_report = _blocked_scene_semantic_report(
                "scene semantics require a passing scene DSL"
            )
        _write_json(work_root / "reports" / "director_scene_semantic_report.json", semantic_report)
        if semantic_report["status"] == "pass":
            converted_resources = _read_json(
                work_root / "reports" / "projectorrays_converted_resources.json"
            )
            try:
                asset_bindings, asset_binding_report = build_asset_binding_ir(
                    detailed,
                    scene_semantics,
                    converted_resources,
                )
            except DirectorAssetBindingError as exc:
                asset_binding_report = _blocked_asset_binding_report(str(exc))
                report["status"] = "blocked"
                report["diagnostics"].extend(asset_binding_report["diagnostics"])
            else:
                _write_json(
                    work_root / "private" / "director_asset_bindings.json",
                    asset_bindings,
                )
        else:
            asset_binding_report = _blocked_asset_binding_report(
                "asset bindings require passing scene semantics"
            )
        _write_json(
            work_root / "reports" / "director_asset_binding_report.json",
            asset_binding_report,
        )
        converted_resources = _read_json(work_root / "reports" / "projectorrays_converted_resources.json")
        try:
            lingo_ir, lingo_report = build_lingo_ir(
                work_root,
                converted_resources,
                transition_cast_root=dict(dump_roots).get("casts"),
            )
        except DirectorLingoError as exc:
            lingo_report = {
                "schema": "tsuinosora.director_lingo_report.v1",
                "status": "blocked",
                "source_resource_count": 0,
                "converted_resource_count": 0,
                "handler_count": 0,
                "source_line_count": 0,
                "encoding_counts": {},
                "statement_counts": {},
                "lingo_ir_sha256": "sha256:" + "0" * 64,
                "diagnostics": [
                    {"code": "TSUI_DIRECTOR_LINGO_BLOCKED", "message": str(exc)}
                ],
                "redaction": {
                    "paths": "alias_or_report_relative_only",
                    "payload": "omitted",
                    "commercial_text": "private_ir_only",
                    "script_source": "private_ir_only",
                },
            }
            report["status"] = "blocked"
            report["diagnostics"].extend(lingo_report["diagnostics"])
        else:
            _write_json(work_root / "private" / "director_lingo_ir.json", lingo_ir)
        _write_json(work_root / "reports" / "director_lingo_report.json", lingo_report)
        # The MENU startMovie handler is authoritative for title audio.  It is
        # available only after the strict Lingo reader succeeds, so rebuild the
        # binding IR here rather than guessing a title asset from filenames.
        if semantic_report["status"] == "pass" and lingo_report["status"] == "pass":
            try:
                asset_bindings, asset_binding_report = build_asset_binding_ir(
                    detailed,
                    scene_semantics,
                    converted_resources,
                    lingo_ir,
                )
            except DirectorAssetBindingError as exc:
                asset_binding_report = _blocked_asset_binding_report(str(exc))
                report["status"] = "blocked"
                report["diagnostics"].extend(asset_binding_report["diagnostics"])
            else:
                _write_json(
                    work_root / "private" / "director_asset_bindings.json",
                    asset_bindings,
                )
            _write_json(
                work_root / "reports" / "director_asset_binding_report.json",
                asset_binding_report,
            )
        if (
            scene_report["status"] == "pass"
            and semantic_report["status"] == "pass"
            and asset_binding_report["status"] == "pass"
            and lingo_report["status"] == "pass"
        ):
            try:
                story_graph, graph_report = build_story_graph(detailed, scene_dsl, lingo_ir)
            except DirectorStoryGraphError as exc:
                graph_report = _blocked_story_graph_report(str(exc))
                report["status"] = "blocked"
                report["diagnostics"].extend(graph_report["diagnostics"])
            else:
                _write_json(work_root / "private" / "director_story_graph.json", story_graph)
        else:
            graph_report = _blocked_story_graph_report(
                "story graph requires passing scene DSL and Lingo IR"
            )
        _write_json(work_root / "reports" / "director_story_graph_report.json", graph_report)
        if graph_report["status"] == "pass" and asset_binding_report["status"] == "pass":
            try:
                story_program, story_program_report = build_story_program_ir(
                    story_graph,
                    asset_bindings,
                )
            except DirectorStoryProgramError as exc:
                story_program_report = _blocked_story_program_report(str(exc))
                report["status"] = "blocked"
                report["diagnostics"].extend(story_program_report["diagnostics"])
            else:
                _write_json(
                    work_root / "private" / "director_story_program.json",
                    story_program,
                )
                try:
                    native_story, native_story_report = build_native_story_ir(
                        story_program,
                        lingo_ir,
                    )
                except DirectorNativeStoryError as exc:
                    native_story_report = {
                        "schema": "tsuinosora.director_native_story_lowering_report.v1",
                        "status": "blocked",
                        "diagnostics": [
                            {
                                "code": "TSUI_DIRECTOR_NATIVE_STORY_BLOCKED",
                                "message": str(exc),
                            }
                        ],
                        "redaction": {
                            "paths": "report_relative_only",
                            "payload": "omitted",
                            "commercial_text": "private_ir_only",
                        },
                    }
                    report["status"] = "blocked"
                    report["diagnostics"].extend(native_story_report["diagnostics"])
                else:
                    _write_json(work_root / "private" / "native_story_ir.json", native_story)
                _write_json(
                    work_root / "reports" / "director_native_story_lowering_report.json",
                    native_story_report,
                )
        else:
            story_program_report = _blocked_story_program_report(
                "story program requires passing graph and asset bindings"
            )
        _write_json(
            work_root / "reports" / "director_story_program_report.json",
            story_program_report,
        )
    _write_json(work_root / "reports" / "director_story_source_report.json", report)
    return report


def _blocked_story_graph_report(message: str) -> dict:
    return {
        "schema": "tsuinosora.director_story_graph_report.v1",
        "status": "blocked",
        "movie_count": 0,
        "node_count": 0,
        "scene_count": 0,
        "choice_count": 0,
        "terminal_count": 0,
        "conditional_node_count": 0,
        "frame_action_binding_count": 0,
        "used_action_script_count": 0,
        "flow_counts": {},
        "story_graph_sha256": "sha256:" + "0" * 64,
        "diagnostics": [{"code": "TSUI_DIRECTOR_STORY_GRAPH_BLOCKED", "message": message}],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
            "script_source": "private_ir_only",
        },
    }


def _blocked_scene_semantic_report(message: str) -> dict:
    return {
        "schema": "tsuinosora.director_scene_semantic_report.v1",
        "status": "blocked",
        "scene_count": 0,
        "source_operation_count": 0,
        "semantic_operation_count": 0,
        "semantic_kind_counts": {},
        "scene_semantic_sha256": "sha256:" + "0" * 64,
        "diagnostics": [{"code": "TSUI_DIRECTOR_SCENE_SEMANTIC_BLOCKED", "message": message}],
        "redaction": {
            "paths": "alias_or_report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
        },
    }


def _blocked_asset_binding_report(message: str) -> dict:
    return {
        "schema": "tsuinosora.director_asset_binding_report.v1",
        "status": "blocked",
        "scene_count": 0,
        "reference_count": 0,
        "unique_asset_count": 0,
        "binding_kind_counts": {},
        "asset_binding_sha256": "sha256:" + "0" * 64,
        "diagnostics": [{"code": "TSUI_DIRECTOR_ASSET_BINDING_BLOCKED", "message": message}],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
            "member_names": "private_ir_only",
        },
    }


def _blocked_story_program_report(message: str) -> dict:
    return {
        "schema": "tsuinosora.director_story_program_report.v1",
        "status": "blocked",
        "movie_count": 0,
        "node_count": 0,
        "source_statement_count": 0,
        "program_operation_count": 0,
        "program_kind_counts": {},
        "story_program_sha256": "sha256:" + "0" * 64,
        "diagnostics": [{"code": "TSUI_DIRECTOR_STORY_PROGRAM_BLOCKED", "message": message}],
        "redaction": {
            "paths": "report_relative_only",
            "payload": "omitted",
            "commercial_text": "private_ir_only",
            "script_source": "private_ir_only",
        },
    }
