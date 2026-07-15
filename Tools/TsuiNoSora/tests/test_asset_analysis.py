import json
import hashlib
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from tsuinosora_tools import (  # noqa: E402
    analyze_assets,
    build_cast_source_map_report,
    build_director_cast_map_report,
    build_director_lingo_map_report,
    build_director_resource_map_report,
    build_source_inventory,
    build_conversion_report,
    build_mount_policy,
    build_modern_profile_report,
    build_route_graph_report,
    build_route_scenarios,
    build_script_source_map_report,
    build_stage3_gate_report,
    build_projectorrays_full_dump_report,
    build_visual_comparison_report,
    build_visual_reference_report,
    build_visual_screenshot_capture_report,
    convert_projectorrays_resources,
    demo_slice_config_template,
    extract_readable_assets,
    import_projectorrays_reader,
    run_internal_demo_bundle,
    run_demo_slice_gate,
    run_local_gate,
    write_demo_slice_config_template,
    write_nativevn_package_input,
    _normalize_visual_capture_image,
    _resolve_visual_capture_launch_command,
    _visual_capture_launch_environment,
)
from projectorrays_json import loads_projectorrays_json  # noqa: E402
from native_story_ir import convert_native_story_ir  # noqa: E402


def native_story_ir_fixture():
    commands = [
        {
            "command_id": "line.opening",
            "handler_id": "handler.start",
            "kind": "text",
            "text": "private opening text",
            "speaker_id": "narrator",
        },
        {
            "command_id": "choice.route",
            "handler_id": "handler.start",
            "kind": "choice",
            "prompt": "private prompt",
            "options": [
                {
                    "option_id": "choice.route.good",
                    "text": "private choice",
                    "target": "ending.good",
                }
            ],
        },
    ]
    return {
        "schema": "tsuinosora.native_story_ir.v1",
        "source_locale": "ja",
        "sources": [
            {
                "source_id": "source.movie.main",
                "relative_path": "data/main.dir",
                "sha256": "a" * 64,
                "kind": "director_movie",
            }
        ],
        "handlers": [
            {
                "handler_id": "handler.start",
                "source_id": "source.movie.main",
                "status": "converted",
            }
        ],
        "stories": [
            {
                "story_id": "main",
                "states": [
                    {"state_id": "opening", "scenes": [{"scene_id": "opening", "commands": commands}]},
                    {
                        "state_id": "ending.good",
                        "scenes": [
                            {
                                "scene_id": "ending.good",
                                "commands": [
                                    {
                                        "command_id": "line.ending.good",
                                        "handler_id": "handler.start",
                                        "kind": "text",
                                        "text": "private ending text",
                                    },
                                    {
                                        "command_id": "jump.ending.good",
                                        "handler_id": "handler.start",
                                        "kind": "jump",
                                        "target": "ending.good",
                                    },
                                ],
                            }
                        ],
                    },
                ],
            }
        ],
        "routes": [
            {
                "route_id": "route.good",
                "terminal_id": "ending.good",
                "choice_ids": ["choice.route.good"],
                "command_ids": ["line.opening", "choice.route", "line.ending.good"],
                "input_events": [
                    {"tick": 0, "event": {"type": "resume"}},
                    {"tick": 1, "event": {"type": "focus", "focused": True}},
                    {
                        "tick": 2,
                        "event": {
                            "type": "keyboard",
                            "physical_key": "Enter",
                            "logical_key": "Enter",
                            "state": "pressed",
                            "repeat": False,
                        },
                    },
                    {"tick": 3, "event": {"type": "shutdown"}},
                ],
            }
        ],
        "coverage": {
            "status": "complete",
            "source_count": 1,
            "handler_count": 1,
            "command_count": 4,
            "route_count": 1,
        },
    }


def write_native_story_ir_fixture(work: Path) -> Path:
    path = work / "private" / "native_story_ir.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(native_story_ir_fixture(), ensure_ascii=False), encoding="utf-8")
    return path


class AssetAnalysisTests(unittest.TestCase):
    def test_native_story_ir_generates_split_story_localization_and_physical_input(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ir = root / "native_story_ir.json"
            output = root / "output"
            ir.write_text(json.dumps(native_story_ir_fixture(), ensure_ascii=False), encoding="utf-8")

            report = convert_native_story_ir(ir, output)
            encoded_report = json.dumps(report, ensure_ascii=False, sort_keys=True)
            story = (output / "Scripts" / "main.astra").read_text(encoding="utf-8")
            localization = json.loads((output / "Localization" / "ja.json").read_text(encoding="utf-8"))
            input_lines = (output / "Automation" / "route.good.jsonl").read_text(encoding="utf-8").splitlines()

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["counts"]["commands"], 4)
            self.assertIn("text key:story.line.opening", story)
            self.assertIn("option key:story.choice.route.option.choice.route.good", story)
            self.assertEqual(localization["strings"]["story.line.opening"], "private opening text")
            self.assertTrue(all(json.loads(line)["schema"] == "astra.user_input_sequence.v1" for line in input_lines))
            self.assertNotIn("private opening text", encoded_report)
            self.assertNotIn(tmp.replace("\\", "/"), encoded_report.replace("\\", "/"))

    def test_native_story_ir_blocks_unknown_command_without_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = native_story_ir_fixture()
            payload["stories"][0]["states"][0]["scenes"][0]["commands"][0]["kind"] = "unknown"
            ir = root / "native_story_ir.json"
            output = root / "output"
            ir.write_text(json.dumps(payload), encoding="utf-8")

            report = convert_native_story_ir(ir, output)

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_NATIVE_STORY_COMMAND_INVALID", {item["code"] for item in report["diagnostics"]})
            self.assertFalse((output / "Scripts").exists())

    def test_native_story_ir_rejects_semantic_shortcut_automation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload = native_story_ir_fixture()
            payload["routes"][0]["input_events"] = [
                {"tick": 0, "event": {"type": "choose", "option_id": "choice.route.good"}},
                {"tick": 1, "event": {"type": "shutdown"}},
            ]
            ir = root / "native_story_ir.json"
            output = root / "output"
            ir.write_text(json.dumps(payload), encoding="utf-8")

            report = convert_native_story_ir(ir, output)

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_NATIVE_STORY_ROUTE_INPUT_INVALID", {item["code"] for item in report["diagnostics"]})

    def test_projectorrays_json_codec_accepts_only_proven_extended_escapes(self):
        value = loads_projectorrays_json(r'{"vertical":"line\vbreak","byte":"\x81\x40","slash":"\\v"}')

        self.assertEqual(value["vertical"], "line\vbreak")
        self.assertEqual(value["byte"], "\u0081@")
        self.assertEqual(value["slash"], "\\v")

    def test_projectorrays_json_codec_rejects_malformed_hex_escape(self):
        with self.assertRaises(json.JSONDecodeError):
            loads_projectorrays_json(r'{"bad":"\xG0"}')

    def test_projectorrays_json_codec_does_not_repair_unrelated_invalid_json(self):
        with self.assertRaises(json.JSONDecodeError):
            loads_projectorrays_json('{"missing": true,}')

    def test_demo_config_template_uses_repo_relative_private_layout(self):
        template = demo_slice_config_template()
        encoded = json.dumps(template, sort_keys=True)

        self.assertEqual(template["schema"], "tsuinosora.demo_slice_config.v1")
        self.assertEqual(template["local_work_root"], "Examples/TsuiNoSora/.local/work")
        self.assertTrue(template["require_full_resource_conversion"])
        self.assertTrue(template["require_visual_screenshot_acceptance"])
        self.assertEqual(template["visual_capture"]["schema"], "tsuinosora.visual_capture_config.v1")
        self.assertEqual(
            template["visual_capture"]["capture_automation"]["schema"],
            "tsuinosora.visual_capture_automation.v1",
        )
        self.assertEqual(template["visual_capture"]["capture_automation"]["backend"], "windows_sendinput")
        self.assertGreaterEqual(len(template["visual_capture"]["checkpoints"]), 2)
        self.assertEqual(len(template["projectorrays_full_dump_roots"]), 3)
        self.assertEqual(
            template["player_automation_report"],
            "Examples/TsuiNoSora/.local/work/reports/live_player_report.json",
        )
        self.assertIn("projectorrays_tool", template)
        self.assertIn("projectorrays_dump_root", template)
        self.assertNotIn("payload", encoded)
        self.assertNotRegex(encoded, r"[A-Za-z]:[\\/]")

    def test_demo_config_template_writer_is_sanitized_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            out = root / "demo.config.json"

            report = write_demo_slice_config_template(out)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.demo_slice_config_template_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertTrue(out.exists())
            self.assertEqual(report["files"][0]["path_alias"], "requested_output")
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

            blocked = write_demo_slice_config_template(out)
            self.assertEqual(blocked["status"], "blocked")
            self.assertTrue(any(diag["code"] == "TSUI_DEMO_CONFIG_TEMPLATE_EXISTS" for diag in blocked["diagnostics"]))

    def test_projectorrays_full_dump_report_blocks_unconverted_binary_chunks(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "movie.dir").write_bytes(b"director")
            (dump / "BITD-1.bin").write_bytes(b"\x82\x00\x82\x00payload")
            (dump / "CASt-1.json").write_text(
                json.dumps({"member": 1, "type": 3, "infoLen": 37, "specificDataLen": 28}),
                encoding="utf-8",
            )
            (dump / "script.ls").write_text("on mouseUp\n  go(label(\"x\"))\nend\n", encoding="utf-8")

            report = build_projectorrays_full_dump_report(work, [("data", dump)])
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.projectorrays_full_dump_report.v1")
            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["counts"]["binary_chunk_count"], 1)
            self.assertEqual(report["resource_coverage"]["converted"], 0)
            self.assertEqual(report["chunk_fourcc_counts"]["BITD"], 1)
            self.assertEqual(report["conversion_plan"][0]["chunk_fourcc"], "BITD")
            self.assertEqual(report["conversion_plan"][0]["role"], "bitmap_or_palette_backed_image")
            self.assertEqual(report["conversion_plan"][0]["status"], "pending_converter")
            self.assertTrue(
                any(diag["code"] == "TSUI_PROJECTORRAYS_FULL_RESOURCE_CONVERSION_REQUIRED" for diag in report["diagnostics"])
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_full_dump_counts_verified_converted_resource_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            native = work / "native-assets" / "images" / "bitd-1.png"
            dump.mkdir()
            source = dump / "BITD-1.bin"
            source.write_bytes(b"\x82\x00\x82\x00payload")
            native.parent.mkdir(parents=True)
            native.write_bytes(make_png(4, 4, fill=(10, 20, 30, 255)))
            (work / "reports").mkdir(parents=True)
            (work / "reports" / "projectorrays_converted_resources.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.projectorrays_converted_resources.v1",
                        "resources": [
                            {
                                "source_alias": "data",
                                "source_relative_path": "BITD-1.bin",
                                "source_sha256": sha256_file(source),
                                "chunk_fourcc": "BITD",
                                "role": "bitmap_or_palette_backed_image",
                                "native_path": "native-assets/images/bitd-1.png",
                                "converted_sha256": sha256_file(native),
                                "byte_size": native.stat().st_size,
                                "conversion_method": "projectorrays_bitd_to_png",
                                "status": "converted",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_projectorrays_full_dump_report(work, [("data", dump)])
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(report["counts"]["converted_resource_count"], 1)
            self.assertEqual(report["conversion_plan"][0]["status"], "converted")
            self.assertEqual(report["converted_resources"][0]["native_path"], "native-assets/images/bitd-1.png")
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))
            self.assertNotIn("commercial text", encoded)

    def test_projectorrays_full_dump_rejects_raw_copy_and_hash_mismatch_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            native = work / "native-assets" / "images" / "bitd-1.bin"
            dump.mkdir()
            source = dump / "BITD-1.bin"
            source.write_bytes(b"\x82\x00\x82\x00payload")
            native.parent.mkdir(parents=True)
            native.write_bytes(source.read_bytes())
            (work / "reports").mkdir(parents=True)
            (work / "reports" / "projectorrays_converted_resources.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.projectorrays_converted_resources.v1",
                        "resources": [
                            {
                                "source_alias": "data",
                                "source_relative_path": "BITD-1.bin",
                                "source_sha256": "sha256:" + ("0" * 64),
                                "chunk_fourcc": "BITD",
                                "role": "bitmap_or_palette_backed_image",
                                "native_path": "native-assets/images/bitd-1.bin",
                                "converted_sha256": sha256_file(native),
                                "byte_size": native.stat().st_size,
                                "conversion_method": "raw_chunk_copy",
                                "status": "converted",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_projectorrays_full_dump_report(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["resource_coverage"]["converted"], 0)
            self.assertIn("TSUI_PROJECTORRAYS_CONVERTED_SOURCE_HASH_MISMATCH", codes)
            self.assertIn("TSUI_PROJECTORRAYS_CONVERTED_METHOD_INVALID", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_writes_sanitized_metadata_evidence_for_json_backed_chunk(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "READY" / "chunks"
            chunks.mkdir(parents=True)
            source = chunks / "CASt-1.bin"
            source.write_bytes(b"\x00\x00\x00\x03\x00\x00\x00\x27metadata")
            (chunks / "CASt-1.json").write_text(
                json.dumps(
                    {
                        "type": 3,
                        "infoLen": 41,
                        "specificDataLen": 28,
                        "info": {
                            "dataOffset": 20,
                            "scriptSrcText": "commercial text must not leak",
                            "name": "HeroName",
                        },
                        "member": {"width": 640, "height": 480},
                    }
                ),
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "READY" / "chunks" / "CASt-1.json"
            native_text = native.read_text(encoding="utf-8")
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["schema"], "tsuinosora.projectorrays_converted_resources.v1")
            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            self.assertEqual(conversion["resources"][0]["source_relative_path"], "READY/chunks/CASt-1.bin")
            self.assertEqual(conversion["resources"][0]["native_path"], "native-assets/projectorrays/data/READY/chunks/CASt-1.json")
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertTrue(native.exists())
            self.assertNotIn("HeroName", native_text)
            self.assertNotIn("commercial text", native_text)
            self.assertNotIn("scriptSrcText", native_text)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_unproven_json_escape(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            source = dump / "CASt-1.bin"
            source.write_bytes(b"\x00\x00\x00\x01metadata")
            (dump / "CASt-1.json").write_text(
                '{\n  "type": 1,\n  "info": {"name": "private", "path": "bad\\escape"}\n}\n',
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertEqual(conversion["converted_count"], 0)
            self.assertIn(
                "TSUI_PROJECTORRAYS_CONVERT_JSON_INVALID",
                {diagnostic["code"] for diagnostic in conversion["diagnostics"]},
            )
            self.assertNotIn("private", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_unconverted_binary_without_json_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "BITD-1.bin").write_bytes(b"\x82\x00\x82\x00payload")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertEqual(conversion["converted_count"], 0)
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_BITD_BINDING_MISSING", codes)
            self.assertEqual(full_dump["resource_coverage"]["converted"], 0)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_decodes_stxt_to_private_text_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            body = "private fixture text".encode("cp932")
            trailer = b"\x00" * 22
            source = dump / "STXT-7.bin"
            source.write_bytes(
                (12).to_bytes(4, "big")
                + len(body).to_bytes(4, "big")
                + len(trailer).to_bytes(4, "big")
                + body
                + trailer
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "STXT-7.txt"
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            self.assertEqual(conversion["resources"][0]["chunk_fourcc"], "STXT")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_stxt_cp932_text")
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(native.read_text(encoding="utf-8"), "private fixture text")
            self.assertNotIn("private fixture text", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_malformed_stxt_header(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "STXT-7.bin").write_bytes(b"\x00\x00\x00\x0c\x00\x00\x00\xff\x00\x00\x00\x16short")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_STXT_INVALID", codes)
            self.assertFalse((work / "native-assets").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_converts_empty_snd_placeholder_to_metadata_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            source = dump / "snd -7.bin"
            source.write_bytes(b"")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "snd_-7.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            self.assertEqual(conversion["resources"][0]["chunk_fourcc"], "snd ")
            self.assertEqual(
                conversion["resources"][0]["conversion_method"],
                "projectorrays_empty_sound_placeholder",
            )
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(native_payload["empty_placeholder"], True)
            self.assertEqual(native_payload["redaction"]["audio"], "omitted")
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_converts_zero_cupt_to_cue_metadata_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            source = dump / "cupt-9.bin"
            source.write_bytes((0).to_bytes(4, "big"))

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "cupt-9.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            self.assertEqual(conversion["resources"][0]["chunk_fourcc"], "cupt")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_cue_point_table")
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(native_payload["cue_point_count"], 0)
            self.assertEqual(native_payload["redaction"]["names"], "omitted")
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_unproven_cupt_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "cupt-9.bin").write_bytes((1).to_bytes(4, "big") + b"\x00\x00")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertEqual(conversion["converted_count"], 0)
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_CUPT_UNPROVEN", codes)
            self.assertFalse((work / "native-assets").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_records_scrf_skipped_reference_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "SCRF-3.bin").write_bytes(b"secret reference payload")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "SCRF-3.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["chunk_fourcc"], "SCRF")
            self.assertEqual(
                conversion["resources"][0]["conversion_method"],
                "projectorrays_scrf_reference_skipped",
            )
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(native_payload["reference_policy"], "skipped_by_director_runtime")
            self.assertNotIn("secret reference payload", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_parses_info_entry_table_without_payload_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            table = (
                (4).to_bytes(4, "big")
                + (2).to_bytes(2, "big")
                + (0).to_bytes(4, "big")
                + (3).to_bytes(4, "big")
                + (7).to_bytes(4, "big")
                + b"onefour"
            )
            (dump / "Cinf-5.bin").write_bytes(table)

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "Cinf-5.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["chunk_fourcc"], "Cinf")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_info_entry_table")
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(native_payload["entry_count"], 2)
            self.assertEqual(native_payload["entry_lengths"], [3, 4])
            self.assertNotIn("onefour", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_malformed_info_entry_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "VWFI-5.bin").write_bytes((99).to_bytes(4, "big") + b"short")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_INFO_TABLE_INVALID", codes)
            self.assertFalse((work / "native-assets").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_parses_sord_score_order_members(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            header = (
                (0).to_bytes(4, "big")
                + (0).to_bytes(4, "big")
                + (2).to_bytes(4, "big")
                + (2).to_bytes(4, "big")
                + (20).to_bytes(2, "big")
                + (4).to_bytes(2, "big")
            )
            (dump / "Sord-8.bin").write_bytes(
                header
                + (1).to_bytes(2, "big")
                + (42).to_bytes(2, "big")
                + (2).to_bytes(2, "big")
                + (7).to_bytes(2, "big")
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "Sord-8.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["chunk_fourcc"], "Sord")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_score_order_table")
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(native_payload["entry_count"], 2)
            self.assertEqual(
                native_payload["referenced_members"],
                [
                    {"cast_library_id": 1, "member_id": 42},
                    {"cast_library_id": 2, "member_id": 7},
                ],
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_parses_fmap_without_font_names(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            name = b"PrivateFont"
            names = len(name).to_bytes(4, "big") + name
            map_body = (
                (0).to_bytes(4, "big")
                + (0).to_bytes(4, "big")
                + (1).to_bytes(4, "big")
                + (1).to_bytes(4, "big")
                + (0).to_bytes(4, "big")
                + (0).to_bytes(4, "big")
                + (0).to_bytes(4, "big")
                + (0).to_bytes(4, "big")
                + (1).to_bytes(2, "big")
                + (7).to_bytes(2, "big")
            )
            (dump / "Fmap-2.bin").write_bytes(len(map_body).to_bytes(4, "big") + len(names).to_bytes(4, "big") + map_body + names)

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "Fmap-2.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_font_map_v4")
            self.assertEqual(native_payload["font_entry_count"], 1)
            self.assertEqual(native_payload["font_entries"][0]["font_id"], 7)
            self.assertEqual(native_payload["font_entries"][0]["name_length"], len(name))
            self.assertNotIn("PrivateFont", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_parses_vwlb_without_label_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            labels = b"alpha\rbeta\r"
            header = (
                (2).to_bytes(2, "big")
                + (1).to_bytes(2, "big")
                + (0).to_bytes(2, "big")
                + (12).to_bytes(2, "big")
                + (6).to_bytes(2, "big")
                + (24).to_bytes(2, "big")
                + len(labels).to_bytes(2, "big")
            )
            (dump / "VWLB-4.bin").write_bytes(header + labels)

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "VWLB-4.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_score_label_table")
            self.assertEqual(native_payload["label_count"], 2)
            self.assertEqual(native_payload["labels"][0]["frame"], 1)
            self.assertEqual(native_payload["labels"][1]["frame"], 12)
            self.assertNotIn("alpha", encoded)
            self.assertNotIn("beta", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_records_fcol_color_table_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "FCOL-6.bin").write_bytes(b"\x00\x01" * 28)

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "FCOL-6.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_fixed_color_table")
            self.assertEqual(native_payload["word_count"], 28)
            self.assertNotIn("0001" * 4, encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_records_fxmp_text_map_without_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "FXmp-6.bin").write_bytes(b"Mac: PrivateFont -> Win: PrivateFont\r\n")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "FXmp-6.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_fxmp_text_map_metadata")
            self.assertEqual(native_payload["line_count"], 1)
            self.assertNotIn("PrivateFont", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_parses_vers_numeric_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "VERS-6.bin").write_bytes(
                (2).to_bytes(2, "big")
                + (2).to_bytes(2, "big")
                + (7).to_bytes(2, "big")
                + (0).to_bytes(2, "big")
                + (2).to_bytes(2, "big")
                + (85).to_bytes(2, "big")
                + (7).to_bytes(2, "big")
                + (0).to_bytes(2, "big")
                + (1).to_bytes(2, "big")
                + (34).to_bytes(2, "big")
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "VERS-6.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_version_table")
            self.assertEqual(native_payload["entry_count"], 2)
            self.assertEqual(native_payload["entries"][0]["director_version"], 7)
            self.assertEqual(native_payload["entries"][0]["major"], 2)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_records_xtrl_without_xtra_names(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            private_name = b"PRIVATE.X32"
            record = (len(private_name) + 1).to_bytes(4, "big") + private_name + b"\x00"
            (dump / "XTRl-6.bin").write_bytes((3).to_bytes(4, "big") + (1).to_bytes(4, "big") + record)

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "XTRl-6.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_xtra_list_metadata")
            self.assertEqual(native_payload["declared_entry_count"], 1)
            self.assertEqual(native_payload["record_count"], 1)
            self.assertNotIn("PRIVATE.X32", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_converts_moa_sound_pair_to_wav_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "AUDIO" / "chunks"
            chunks.mkdir(parents=True)
            key = chunks / "KEY_-3.bin"
            key.write_bytes(
                (12).to_bytes(2, "little")
                + (12).to_bytes(2, "little")
                + (0).to_bytes(4, "little")
                + (2).to_bytes(4, "little")
                + (10).to_bytes(4, "little")
                + (200).to_bytes(4, "little")
                + int.from_bytes(b"sndH", "big").to_bytes(4, "little")
                + (11).to_bytes(4, "little")
                + (200).to_bytes(4, "little")
                + int.from_bytes(b"sndS", "big").to_bytes(4, "little")
            )
            (chunks / "KEY_-3.json").write_text("{}", encoding="utf-8")
            header_fields = [0, 4, 0, 0, 0, 0, 0, 0, 4, 2, 2, 22050, 44100]
            (chunks / "sndH-10.bin").write_bytes(
                b"".join(value.to_bytes(4, "big", signed=True) for value in header_fields)
                + (b"\x00" * 16)
                + (16).to_bytes(4, "big", signed=True)
                + (2).to_bytes(4, "big", signed=True)
                + (1).to_bytes(4, "big", signed=True)
                + (2).to_bytes(4, "big", signed=True)
                + (b"\x00" * 16)
            )
            (chunks / "sndS-11.bin").write_bytes(b"\x00\x01\xff\xfe")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            header_native = work / "native-assets" / "projectorrays" / "data" / "AUDIO" / "chunks" / "sndH-10.json"
            wav_native = work / "native-assets" / "projectorrays" / "data" / "AUDIO" / "chunks" / "sndS-11.wav"
            header_payload = json.loads(header_native.read_text(encoding="utf-8"))
            wav = wav_native.read_bytes()
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 3)
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 3, "converted": 3})
            self.assertEqual(header_payload["sample_resource_id"], 11)
            self.assertEqual(header_payload["sample_rate"], 22050)
            self.assertEqual(wav[:4], b"RIFF")
            self.assertEqual(wav[8:12], b"WAVE")
            self.assertEqual(int.from_bytes(wav[24:28], "little"), 22050)
            self.assertEqual(wav[-4:], b"\x01\x00\xfe\xff")
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_unbound_snds_sample(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "sndS-11.bin").write_bytes(b"\x00\x01\xff\xfe")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_SOUND_BINDING_MISSING", codes)
            self.assertFalse((work / "native-assets").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_extracts_bound_edim_macrz_mp3_stream(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "AUDIO" / "chunks"
            chunks.mkdir(parents=True)
            (chunks / "KEY_-3.bin").write_bytes(
                (12).to_bytes(2, "little")
                + (12).to_bytes(2, "little")
                + (0).to_bytes(4, "little")
                + (1).to_bytes(4, "little")
                + (21).to_bytes(4, "little")
                + (200).to_bytes(4, "little")
                + int.from_bytes(b"ediM", "big").to_bytes(4, "little")
            )
            (chunks / "KEY_-3.json").write_text("{}", encoding="utf-8")
            (chunks / "CASt-200.bin").write_bytes(
                (6).to_bytes(4, "big") + (0).to_bytes(4, "big") + (0).to_bytes(4, "big")
            )
            (chunks / "CASt-200.json").write_text(
                json.dumps({"type": 6, "info": {"name": "private sound name"}, "member": {}}),
                encoding="utf-8",
            )
            frame = bytes.fromhex("fff38054") + bytes(204)
            header_words = [320, 3, 22050, 64000, 1500, 16, 0, 0]
            (chunks / "ediM-21.bin").write_bytes(
                b"".join(value.to_bytes(4, "big") for value in header_words)
                + (2).to_bytes(2, "big")
                + (2).to_bytes(2, "big")
                + b"MACRZ"
                + bytes(range(16))
                + b"Copyright Macromedia Inc 1996-1997"
                + bytes(233)
                + frame
                + frame
                + frame
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            resource = next(item for item in conversion["resources"] if item["chunk_fourcc"] == "ediM")
            native = work / "native-assets" / "projectorrays" / "data" / "AUDIO" / "chunks" / "ediM-21.mp3"
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 3)
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 3, "converted": 3})
            self.assertEqual(resource["chunk_fourcc"], "ediM")
            self.assertEqual(resource["conversion_method"], "projectorrays_edim_macrz_mp3_extract")
            self.assertEqual(resource["native_path"], "native-assets/projectorrays/data/AUDIO/chunks/ediM-21.mp3")
            self.assertEqual(resource["media_codec"], "mp3")
            self.assertEqual(resource["media_stream_offset"], 324)
            self.assertEqual(resource["frame_count"], 3)
            self.assertEqual(resource["sample_rate"], 22050)
            self.assertEqual(resource["bitrate_kbps"], 64)
            self.assertEqual(resource["channel_count"], 2)
            self.assertEqual(native.read_bytes(), frame + frame + frame)
            self.assertNotIn("private sound name", encoded)
            self.assertNotIn("Copyright Macromedia", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_bound_edim_macrz_without_verified_mp3_stream(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "AUDIO" / "chunks"
            chunks.mkdir(parents=True)
            (chunks / "KEY_-3.bin").write_bytes(
                (12).to_bytes(2, "little")
                + (12).to_bytes(2, "little")
                + (0).to_bytes(4, "little")
                + (1).to_bytes(4, "little")
                + (21).to_bytes(4, "little")
                + (200).to_bytes(4, "little")
                + int.from_bytes(b"ediM", "big").to_bytes(4, "little")
            )
            (chunks / "KEY_-3.json").write_text("{}", encoding="utf-8")
            (chunks / "CASt-200.bin").write_bytes(
                (6).to_bytes(4, "big") + (0).to_bytes(4, "big") + (0).to_bytes(4, "big")
            )
            (chunks / "CASt-200.json").write_text(
                json.dumps({"type": 6, "info": {"name": "private sound name"}, "member": {}}),
                encoding="utf-8",
            )
            header_words = [320, 3, 22050, 64000, 1500, 16, 0, 0]
            (chunks / "ediM-21.bin").write_bytes(
                b"".join(value.to_bytes(4, "big") for value in header_words)
                + (2).to_bytes(2, "big")
                + (2).to_bytes(2, "big")
                + b"MACRZ"
                + bytes(range(16))
                + b"private-audio-payload"
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            diagnostics = conversion["diagnostics"]
            codes = {diagnostic["code"] for diagnostic in diagnostics}
            diagnostic = next(
                (
                    item
                    for item in diagnostics
                    if item["code"] == "TSUI_PROJECTORRAYS_CONVERT_EDIM_MACRZ_MP3_STREAM_INVALID"
                ),
                None,
            )
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_EDIM_MACRZ_MP3_STREAM_INVALID", codes)
            self.assertNotIn("TSUI_PROJECTORRAYS_CONVERT_UNSUPPORTED_CHUNK", codes)
            self.assertIsNotNone(diagnostic)
            self.assertEqual(diagnostic["parent_resource_id"], 200)
            self.assertEqual(diagnostic["parent_member_type"], 6)
            self.assertEqual(diagnostic["codec_marker"], "MACRZ")
            self.assertEqual(diagnostic["macrz_signature_offset"], 36)
            self.assertEqual(diagnostic["header_u32_words"], header_words)
            self.assertNotIn("private sound name", encoded)
            self.assertNotIn("private-audio-payload", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_decodes_vwsc_score_frames_without_frame_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            frame = (
                (7).to_bytes(2, "big")
                + (1).to_bytes(2, "big")
                + (54).to_bytes(2, "big")
                + (10).to_bytes(1, "big")
            )
            detail0 = (
                (20 + len(frame)).to_bytes(4, "big")
                + (20).to_bytes(4, "big")
                + (1).to_bytes(4, "big")
                + (13).to_bytes(2, "big")
                + (48).to_bytes(2, "big")
                + (120).to_bytes(2, "big")
                + (1).to_bytes(2, "big")
                + frame
            )
            detail1 = b"private-frame-data"
            payload = (
                (0).to_bytes(4, "big")
                + (-3).to_bytes(4, "big", signed=True)
                + (12).to_bytes(4, "big")
                + (2).to_bytes(4, "big")
                + (3).to_bytes(4, "big")
                + len(detail1).to_bytes(4, "big")
                + (0).to_bytes(4, "big")
                + len(detail0).to_bytes(4, "big")
                + (len(detail0) + len(detail1)).to_bytes(4, "big")
                + detail0
                + detail1
            )
            payload = len(payload).to_bytes(4, "big") + payload[4:]
            (dump / "VWSC-6.bin").write_bytes(payload)

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "VWSC-6.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_vwsc_score_metadata")
            self.assertEqual(native_payload["detail_entry_count"], 2)
            self.assertEqual(native_payload["score_header"]["num_frames"], 1)
            self.assertEqual(native_payload["score_ir"]["decoded_frame_count"], 1)
            self.assertEqual(native_payload["score_ir"]["frames"][0]["main"]["tempo"], 10)
            self.assertNotIn("private-frame-data", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_records_xmed_metadata_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            dump.mkdir()
            (dump / "XMED-9.bin").write_bytes(b"FFFF000000060004private-xtra-name")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "XMED-9.json"
            native_payload = json.loads(native.read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True) + native.read_text(encoding="utf-8")

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["resources"][0]["conversion_method"], "projectorrays_xmed_metadata")
            self.assertEqual(native_payload["format_marker"], "FFFF0000")
            self.assertNotIn("private-xtra-name", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_maps_lscr_cast_member_to_private_script_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            scripts = dump / "MOVIE" / "casts" / "Internal"
            chunks.mkdir(parents=True)
            scripts.mkdir(parents=True)
            source = chunks / "Lscr-100.bin"
            source.write_bytes(b"\x01\x02\x03\x04")
            (chunks / "Lscr-100.json").write_text(
                json.dumps({"scriptNumber": 7, "castID": (1 << 16) + 42}),
                encoding="utf-8",
            )
            (scripts / "BehaviorScript 42 - private_name.ls").write_text(
                "private script text",
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "MOVIE" / "chunks" / "Lscr-100.ls"
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            self.assertEqual(conversion["resources"][0]["chunk_fourcc"], "Lscr")
            self.assertEqual(
                conversion["resources"][0]["conversion_method"],
                "projectorrays_lscr_decompiled_script",
            )
            self.assertEqual(conversion["resources"][0]["cast_member_id"], 42)
            self.assertEqual(conversion["resources"][0]["script_number"], 7)
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertEqual(native.read_text(encoding="utf-8"), "private script text")
            self.assertNotIn("private script text", encoded)
            self.assertNotIn("private_name", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_maps_lscr_source_without_display_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            scripts = dump / "MOVIE" / "casts" / "Internal"
            chunks.mkdir(parents=True)
            scripts.mkdir(parents=True)
            source = chunks / "Lscr-100.bin"
            source.write_bytes(b"\x01\x02\x03\x04")
            (chunks / "Lscr-100.json").write_text(
                json.dumps({"scriptNumber": 7, "castID": (1 << 16) + 42}),
                encoding="utf-8",
            )
            (scripts / "BehaviorScript 42.ls").write_text("private script text", encoding="utf-8")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "MOVIE" / "chunks" / "Lscr-100.ls"
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            self.assertEqual(conversion["resources"][0]["cast_member_id"], 42)
            self.assertEqual(native.read_text(encoding="utf-8"), "private script text")
            self.assertNotIn("private script text", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_falls_back_to_lscr_script_number_binding(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MENU" / "chunks"
            scripts = dump / "MENU" / "casts" / "Internal"
            chunks.mkdir(parents=True)
            scripts.mkdir(parents=True)
            source = chunks / "Lscr-421.bin"
            source.write_bytes(b"\x01\x02\x03\x04")
            (chunks / "Lscr-421.json").write_text(
                json.dumps({"scriptNumber": 21, "castID": (1 << 16) + 87}),
                encoding="utf-8",
            )
            (scripts / "BehaviorScript 21 - private_menu_action.ls").write_text(
                "private script text",
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            resource = conversion["resources"][0]
            self.assertEqual(resource["chunk_fourcc"], "Lscr")
            self.assertEqual(resource["cast_member_id"], 87)
            self.assertEqual(resource["script_number"], 21)
            self.assertEqual(resource["script_source_binding"], "script_number")
            self.assertNotIn("private script text", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_maps_lscr_cast_and_parent_script_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MENU" / "chunks"
            menu_scripts = dump / "MENU" / "casts" / "Internal"
            global_chunks = dump / "GLOBALS" / "chunks"
            global_scripts = dump / "GLOBALS" / "casts" / "External"
            chunks.mkdir(parents=True)
            menu_scripts.mkdir(parents=True)
            global_chunks.mkdir(parents=True)
            global_scripts.mkdir(parents=True)
            (chunks / "Lscr-409.bin").write_bytes(b"\x01\x02\x03\x04")
            (chunks / "Lscr-409.json").write_text(
                json.dumps({"scriptNumber": 9, "castID": (1 << 16) + 61}),
                encoding="utf-8",
            )
            (menu_scripts / "CastScript 61 - private_menu_cast.ls").write_text(
                "private cast script text",
                encoding="utf-8",
            )
            (global_chunks / "Lscr-232.bin").write_bytes(b"\x05\x06\x07\x08")
            (global_chunks / "Lscr-232.json").write_text(
                json.dumps({"scriptNumber": 32, "castID": (1 << 16) + 25}),
                encoding="utf-8",
            )
            (global_scripts / "ParentScript 25 - private_parent.ls").write_text(
                "private parent script text",
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            kinds = {resource["script_source_kind"] for resource in conversion["resources"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 2)
            self.assertEqual(kinds, {"castscript", "parentscript"})
            self.assertNotIn("private cast script text", encoded)
            self.assertNotIn("private parent script text", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_malformed_lscr_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            scripts = dump / "MOVIE" / "casts" / "Internal"
            chunks.mkdir(parents=True)
            scripts.mkdir(parents=True)
            (chunks / "Lscr-500.bin").write_bytes(b"\x01\x02\x03\x04")
            (chunks / "Lscr-500.json").write_text(
                '{"scriptNumber": 7, "castID": 65578, "scriptText": "private\\qscript"}',
                encoding="utf-8",
            )
            (scripts / "BehaviorScript 42 - private_behavior.ls").write_text(
                "private script text",
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertEqual(conversion["converted_count"], 0)
            self.assertIn(
                "TSUI_PROJECTORRAYS_CONVERT_LSCR_METADATA_INVALID",
                {diagnostic["code"] for diagnostic in conversion["diagnostics"]},
            )
            self.assertNotIn("private\\qscript", encoded)
            self.assertNotIn("private script text", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_ambiguous_lscr_script_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            internal = dump / "MOVIE" / "casts" / "Internal"
            external = dump / "MOVIE" / "casts" / "External"
            chunks.mkdir(parents=True)
            internal.mkdir(parents=True)
            external.mkdir(parents=True)
            (chunks / "Lscr-100.bin").write_bytes(b"\x01\x02\x03\x04")
            (chunks / "Lscr-100.json").write_text(
                json.dumps({"scriptNumber": 7, "castID": (1 << 16) + 42}),
                encoding="utf-8",
            )
            (internal / "BehaviorScript 42 - a.ls").write_text("secret_alpha_payload", encoding="utf-8")
            (external / "BehaviorScript 42 - b.ls").write_text("secret_beta_payload", encoding="utf-8")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertEqual(conversion["converted_count"], 0)
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_LSCR_SOURCE_AMBIGUOUS", codes)
            self.assertFalse((work / "native-assets").exists())
            self.assertNotIn("secret_alpha_payload", encoded)
            self.assertNotIn("secret_beta_payload", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_empty_lscr_script_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            scripts = dump / "MOVIE" / "casts" / "Internal"
            chunks.mkdir(parents=True)
            scripts.mkdir(parents=True)
            (chunks / "Lscr-100.bin").write_bytes(b"\x01\x02\x03\x04")
            (chunks / "Lscr-100.json").write_text(
                json.dumps({"scriptNumber": 7, "castID": (1 << 16) + 42}),
                encoding="utf-8",
            )
            (scripts / "BehaviorScript 42 - empty.ls").write_text("", encoding="utf-8")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertEqual(conversion["converted_count"], 0)
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_LSCR_SOURCE_EMPTY", codes)
            self.assertFalse((work / "native-assets").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_converts_empty_lscr_metadata_without_source_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            chunks.mkdir(parents=True)
            (chunks / "Lscr-100.bin").write_bytes(b"\x00" * 92)
            (chunks / "Lscr-100.json").write_text(
                json.dumps(
                    {
                        "scriptNumber": 47,
                        "castID": (1 << 16) + 2,
                        "scriptFlags": 0,
                        "handlersCount": 0,
                        "literalsCount": 0,
                        "globalsCount": 0,
                        "propertiesCount": 0,
                    }
                ),
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 1)
            lingo = next(item for item in conversion["resources"] if item["chunk_fourcc"] == "Lscr")
            self.assertEqual(lingo["conversion_method"], "projectorrays_lscr_empty_script_metadata")
            self.assertEqual(lingo["script_source_binding"], "empty_script_metadata")
            self.assertEqual(lingo["handler_count"], 0)
            self.assertEqual(lingo["literal_count"], 0)
            self.assertTrue((work / lingo["native_path"]).is_file())
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 1, "converted": 1})
            self.assertNotIn("script_text", encoded)
            self.assertNotIn("source_text", encoded)
            self.assertNotIn('"bytecode":', encoded)
            self.assertNotIn("raw_payload", encoded)
            self.assertNotIn("source_payload", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_decodes_bitd_32bpp_to_png_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            chunks.mkdir(parents=True)
            key = chunks / "KEY_-3.bin"
            key.write_bytes(
                (12).to_bytes(2, "little")
                + (12).to_bytes(2, "little")
                + (1).to_bytes(4, "little")
                + (1).to_bytes(4, "little")
                + (200).to_bytes(4, "little")
                + (100).to_bytes(4, "little")
                + int.from_bytes(b"BITD", "big").to_bytes(4, "little")
            )
            (chunks / "KEY_-3.json").write_text("{}", encoding="utf-8")
            (chunks / "DRCF-9.bin").write_bytes(b"director config")
            (chunks / "DRCF-9.json").write_text(json.dumps({"directorVersion": 1406}), encoding="utf-8")
            spec = (
                (0x8008).to_bytes(2, "big")
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big", signed=True)
                + (1).to_bytes(2, "big", signed=True)
                + (2).to_bytes(2, "big", signed=True)
                + b"\x00\x00"
                + (0).to_bytes(2, "big")
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big")
                + (0).to_bytes(2, "big")
                + b"\x00"
                + b"\x20"
                + (-1).to_bytes(2, "big", signed=True)
                + (-1).to_bytes(2, "big", signed=True)
            )
            (chunks / "CASt-100.bin").write_bytes(
                (1).to_bytes(4, "big") + (0).to_bytes(4, "big") + len(spec).to_bytes(4, "big") + spec
            )
            (chunks / "CASt-100.json").write_text(json.dumps({"type": 1, "info": {}, "member": {}}), encoding="utf-8")
            source = chunks / "BITD-200.bin"
            source.write_bytes(bytes([0, 10, 20, 30, 0, 40, 50, 60]))

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            full_dump = build_projectorrays_full_dump_report(work, [("data", dump)])
            native = work / "native-assets" / "projectorrays" / "data" / "MOVIE" / "chunks" / "BITD-200.png"
            png = native.read_bytes()
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            self.assertEqual(conversion["converted_count"], 4)
            bitd = next(item for item in conversion["resources"] if item["chunk_fourcc"] == "BITD")
            self.assertEqual(bitd["conversion_method"], "projectorrays_bitd_rgba_png")
            self.assertEqual(bitd["width"], 2)
            self.assertEqual(bitd["height"], 1)
            self.assertEqual(bitd["bits_per_pixel"], 32)
            self.assertEqual(full_dump["resource_coverage"], {"status": "pass", "required": 4, "converted": 4})
            self.assertEqual(png[:8], b"\x89PNG\r\n\x1a\n")
            self.assertEqual(int.from_bytes(png[16:20], "big"), 2)
            self.assertEqual(int.from_bytes(png[20:24], "big"), 1)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_blocks_bitd_palette_backed_without_palette(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            chunks.mkdir(parents=True)
            (chunks / "KEY_-3.bin").write_bytes(
                (12).to_bytes(2, "little")
                + (12).to_bytes(2, "little")
                + (1).to_bytes(4, "little")
                + (1).to_bytes(4, "little")
                + (200).to_bytes(4, "little")
                + (100).to_bytes(4, "little")
                + int.from_bytes(b"BITD", "big").to_bytes(4, "little")
            )
            (chunks / "KEY_-3.json").write_text("{}", encoding="utf-8")
            (chunks / "DRCF-9.bin").write_bytes(b"director config")
            (chunks / "DRCF-9.json").write_text(json.dumps({"directorVersion": 1406}), encoding="utf-8")
            spec = (
                (0x8002).to_bytes(2, "big")
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big", signed=True)
                + (1).to_bytes(2, "big", signed=True)
                + (2).to_bytes(2, "big", signed=True)
                + b"\x00\x00"
                + (0).to_bytes(2, "big")
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big")
                + (0).to_bytes(2, "big")
                + b"\x00"
                + b"\x08"
                + (-1).to_bytes(2, "big", signed=True)
                + (-1).to_bytes(2, "big", signed=True)
            )
            (chunks / "CASt-100.bin").write_bytes(
                (1).to_bytes(4, "big") + (0).to_bytes(4, "big") + len(spec).to_bytes(4, "big") + spec
            )
            (chunks / "CASt-100.json").write_text(json.dumps({"type": 1, "info": {}, "member": {}}), encoding="utf-8")
            (chunks / "BITD-200.bin").write_bytes(b"\x00\x01")

            conversion = convert_projectorrays_resources(work, [("data", dump)])
            codes = {diagnostic["code"] for diagnostic in conversion["diagnostics"]}
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "blocked")
            self.assertIn("TSUI_PROJECTORRAYS_CONVERT_BITD_PALETTE_REQUIRED", codes)
            self.assertFalse((work / "native-assets" / "projectorrays" / "data" / "MOVIE" / "chunks" / "BITD-200.png").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_converter_decodes_8bpp_bitd_with_palette_sidecar(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dump = root / "full-dump"
            work = root / "work"
            chunks = dump / "MOVIE" / "chunks"
            chunks.mkdir(parents=True)
            (chunks / "KEY_-3.bin").write_bytes(
                (12).to_bytes(2, "little")
                + (12).to_bytes(2, "little")
                + (1).to_bytes(4, "little")
                + (1).to_bytes(4, "little")
                + (200).to_bytes(4, "little")
                + (100).to_bytes(4, "little")
                + int.from_bytes(b"BITD", "big").to_bytes(4, "little")
            )
            (chunks / "KEY_-3.json").write_text("{}", encoding="utf-8")
            (chunks / "DRCF-9.bin").write_bytes(b"director config")
            (chunks / "DRCF-9.json").write_text(json.dumps({"directorVersion": 1406}), encoding="utf-8")
            spec = (
                (0x8002).to_bytes(2, "big")
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big", signed=True)
                + (1).to_bytes(2, "big", signed=True)
                + (2).to_bytes(2, "big", signed=True)
                + b"\x00\x00"
                + (0).to_bytes(2, "big")
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big", signed=True)
                + (0).to_bytes(2, "big")
                + (0).to_bytes(2, "big")
                + b"\x00"
                + b"\x08"
                + (-1).to_bytes(2, "big", signed=True)
                + (-101).to_bytes(2, "big", signed=True)
            )
            (chunks / "CASt-100.bin").write_bytes(
                (1).to_bytes(4, "big") + (0).to_bytes(4, "big") + len(spec).to_bytes(4, "big") + spec
            )
            (chunks / "CASt-100.json").write_text(json.dumps({"type": 1, "info": {}, "member": {}}), encoding="utf-8")
            (chunks / "BITD-200.bin").write_bytes(b"\x01\x01\x02")
            palette = [[0, 0, 0] for _ in range(256)]
            palette[1] = [255, 0, 0]
            palette[2] = [0, 255, 0]
            palette_sidecar = root / "palette.json"
            palette_sidecar.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.projectorrays_palette_sidecar.v1",
                        "palettes": [
                            {
                                "id": "synthetic_system_win_d5",
                                "stored_clut_id": -101,
                                "director_palette_id": -102,
                                "colors": palette,
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            conversion = convert_projectorrays_resources(
                work,
                [("data", dump)],
                palette_sidecars=[palette_sidecar],
            )
            native = work / "native-assets" / "projectorrays" / "data" / "MOVIE" / "chunks" / "BITD-200.png"
            encoded = json.dumps(conversion, sort_keys=True)

            self.assertEqual(conversion["status"], "pass")
            bitd = next(item for item in conversion["resources"] if item["chunk_fourcc"] == "BITD")
            self.assertEqual(bitd["conversion_method"], "projectorrays_bitd_palette_png")
            self.assertEqual(bitd["palette_id"], "synthetic_system_win_d5")
            self.assertEqual(bitd["stored_clut_id"], -101)
            self.assertEqual(native.read_bytes()[:8], b"\x89PNG\r\n\x1a\n")
            self.assertNotIn("colors", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_reader_import_writes_sanitized_sidecar_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tool = root / "tools" / "projectorrays"
            dump = root / "projectorrays-dump"
            work = root / "work"
            tool.parent.mkdir()
            dump.mkdir()
            tool.write_bytes(b"projectorrays fixture")
            (dump / "scripts").mkdir()
            (dump / "scripts" / "main.lingo").write_text(
                "-- astra route: classic.main -> ending.good choices: choice.start\n"
                "put \"commercial text omitted\"\n",
                encoding="utf-8",
            )
            config = root / "reader.config.json"
            config.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.projectorrays_reader_config.v1",
                        "projectorrays_tool": str(tool),
                        "dump_root": str(dump),
                        "local_work_root": str(work),
                    }
                ),
                encoding="utf-8",
            )

            report = import_projectorrays_reader(config)
            source_map = json.loads(
                (work / "unpacked" / "projectorrays_script_source_map.json").read_text(encoding="utf-8")
            )
            script_report = build_script_source_map_report(work / "unpacked")
            encoded = json.dumps({"reader": report, "source_map": source_map, "script": script_report}, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.projectorrays_reader_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(source_map["schema"], "tsuinosora.script_source_map.v1")
            self.assertEqual(source_map["reader"]["tool_id"], "projectorrays")
            self.assertTrue(source_map["reader"]["tool_hash"].startswith("sha256:"))
            self.assertEqual(script_report["status"], "pass")
            self.assertEqual(script_report["routes"][0]["route_id"], "classic.main")
            self.assertNotIn("commercial text omitted", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_projectorrays_reader_derives_routes_from_go_script_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            tool = root / "tools" / "projectorrays"
            dump = root / "projectorrays-dump"
            work = root / "work"
            script_dir = dump / "K" / "casts" / "Internal"
            tool.parent.mkdir()
            script_dir.mkdir(parents=True)
            tool.write_bytes(b"projectorrays fixture")
            (script_dir / "BehaviorScript 105 - GO[1321].ls").write_text(
                "put \"commercial route script omitted\"\n",
                encoding="utf-8",
            )
            config = root / "reader.config.json"
            config.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.projectorrays_reader_config.v1",
                        "projectorrays_tool": str(tool),
                        "dump_root": str(dump),
                        "local_work_root": str(work),
                    }
                ),
                encoding="utf-8",
            )

            report = import_projectorrays_reader(config)
            source_map = json.loads(
                (work / "unpacked" / "projectorrays_script_source_map.json").read_text(encoding="utf-8")
            )
            script_report = build_script_source_map_report(work / "unpacked")
            encoded = json.dumps({"reader": report, "source_map": source_map, "script": script_report}, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(source_map["routes"][0]["route_id"], "classic.1321")
            self.assertEqual(script_report["status"], "pass")
            self.assertEqual(script_report["routes"][0]["terminal"], "ending.classic_1321")
            self.assertEqual(script_report["routes"][0]["choices"], [])
            self.assertNotIn("commercial route script omitted", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_inventory_uses_alias_and_relative_paths_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "source"
            root.mkdir()
            (root / "DATA").mkdir()
            (root / "DATA" / "bg.png").write_bytes(make_png(4, 4, fill=(1, 2, 3, 255)))

            report = build_source_inventory(root, "original_install_root")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.source_inventory.v1")
            self.assertEqual(report["root_alias"], "original_install_root")
            self.assertIn("DATA/bg.png", [entry["relative_path"] for entry in report["files"]])
            self.assertEqual(report["format_counts"]["image_png"], 1)
            self.assertFalse(report["edition_fingerprint"]["ready_dxr_present"])
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_copies_sidecars_without_path_leak(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "source"
            work = root / "work"
            (source / "Assets").mkdir(parents=True)
            (source / "Scripts").mkdir()
            (source / "Assets" / "bg.png").write_bytes(make_png(4, 4, fill=(1, 2, 3, 255)))
            (source / "Scripts" / "main.astra").write_text(
                "show background Assets/bg.png\n",
                encoding="utf-8",
            )

            report = extract_readable_assets(source, work, "original_install_root")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.extract_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["extracted_count"], 2)
            self.assertEqual(report["protected_container_count"], 0)
            self.assertEqual(report["files"][0]["output_relative_path"], "unpacked/Assets/bg.png")
            self.assertTrue((work / "unpacked" / "Assets" / "bg.png").exists())
            self.assertTrue((work / "reports" / "extract_report.json").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_blocks_unparsed_director_containers(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            (source / "DATA").mkdir(parents=True)
            (source / "Assets").mkdir()
            (source / "READY.dxr").write_bytes(b"synthetic director container")
            (source / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (source / "Assets" / "bg.png").write_bytes(make_png(4, 4, fill=(1, 2, 3, 255)))

            report = extract_readable_assets(source, work, "original_install_root")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["extracted_count"], 1)
            self.assertEqual(report["protected_container_count"], 2)
            self.assertIn(
                "TSUI_EXTRACT_DIRECTOR_READER_REQUIRED",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertTrue((work / "unpacked" / "Assets" / "bg.png").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_reads_unprotected_riff_container_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            (source / "READY.dxr").write_bytes(
                make_riff_container([("PNG ", make_png(4, 4, fill=(1, 2, 3, 255)))])
            )

            report = extract_readable_assets(source, work, "original_install_root")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["container_count"], 1)
            self.assertEqual(report["container_entry_count"], 1)
            self.assertEqual(report["protected_container_count"], 0)
            self.assertEqual(report["containers"][0]["container_format"], "RIFF")
            self.assertEqual(report["containers"][0]["entries"][0]["coverage_status"], "extracted")
            self.assertEqual(report["files"][0]["relative_path"], "containers/ready/0001_png.png")
            self.assertTrue((work / "unpacked" / "containers" / "ready" / "0001_png.png").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_uses_director_resource_map_not_dead_chunks(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            script = "-- astra route: classic.main -> ending.good choices: choice.start\n"
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("PNG ", make_png(4, 4, fill=(1, 2, 3, 255))),
                        ("Lscr", script.encode("utf-8")),
                    ],
                    dead_chunks=[("PNG ", make_png(4, 4, fill=(255, 0, 0, 255)))],
                )
            )

            map_report = build_director_resource_map_report(source)
            report = extract_readable_assets(source, work, "original_install_root")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(map_report["schema"], "tsuinosora.director_resource_map.v1")
            self.assertEqual(map_report["status"], "pass")
            self.assertEqual(map_report["tag_counts"]["PNG "], 1)
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["containers"][0]["extraction_mode"], "director_resource_map")
            self.assertEqual(report["containers"][0]["readable_payload_count"], 2)
            self.assertEqual(
                [entry["relative_path"] for entry in report["files"]],
                ["containers/ready/0001_png.png", "containers/ready/0002_lscr.ls"],
            )
            self.assertNotIn("classic.main -> ending.good", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_director_resource_map_ignores_free_mmap_entries(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container_with_free_entry(
                    [("PNG ", make_png(4, 4, fill=(1, 2, 3, 255)))]
                )
            )

            report = build_director_resource_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.director_resource_map.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["containers"][0]["resource_count"], 1)
            self.assertEqual(report["containers"][0]["free_resource_count"], 1)
            self.assertEqual(report["containers"][0]["tag_counts"]["PNG "], 1)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_blocks_broken_director_resource_map_without_fallback(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            broken = bytearray(
                make_director_mapped_container([("PNG ", make_png(4, 4, fill=(1, 2, 3, 255)))])
            )
            broken[24:28] = (999999).to_bytes(4, "little")
            (source / "READY.dxr").write_bytes(bytes(broken))

            report = extract_readable_assets(source, work, "original_install_root")

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["containers"][0]["readable_payload_count"], 0)
            self.assertEqual(report["containers"][0]["extraction_mode"], "director_resource_map")
            self.assertIn(
                "TSUI_DIRECTOR_RESOURCE_MAP_MMAP_OFFSET_INVALID",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )

    def test_extract_readable_assets_blocks_director_container_declared_size_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            readable = make_director_mapped_container(
                [("PNG ", make_png(4, 4, fill=(1, 2, 3, 255)))]
            )
            (source / "READY.dxr").write_bytes(readable + b"extra")

            report = extract_readable_assets(source, work, "original_install_root")
            map_report = build_director_resource_map_report(source)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(map_report["status"], "blocked")
            self.assertEqual(map_report["containers"][0]["resource_count"], 0)
            self.assertEqual(map_report["containers"][0]["tag_counts"], {})
            self.assertIn(
                "TSUI_DIRECTOR_RESOURCE_MAP_SIZE_MISMATCH",
                {diagnostic["code"] for diagnostic in map_report["diagnostics"]},
            )
            self.assertFalse((work / "unpacked" / "containers" / "ready" / "0001_png.png").exists())

    def test_extract_readable_assets_does_not_write_payload_from_size_mismatch_linear_riff(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            (source / "READY.dir").write_bytes(
                make_riff_container([("PNG ", make_png(4, 4, fill=(1, 2, 3, 255)))]) + b"extra"
            )

            report = extract_readable_assets(source, work, "original_install_root")

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["containers"][0]["readable_payload_count"], 0)
            self.assertIn(
                "TSUI_EXTRACT_CONTAINER_SIZE_MISMATCH",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertFalse((work / "unpacked" / "containers" / "ready" / "0001_png.png").exists())

    def test_extract_readable_assets_reads_verified_xfir_riff_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            readable = make_director_mapped_container(
                [("PNG ", make_png(4, 4, fill=(1, 2, 3, 255)))]
            )
            (source / "READY.dcr").write_bytes(make_xfir_wrapped_riff(readable))

            report = extract_readable_assets(source, work, "original_install_root")
            map_report = build_director_resource_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["containers"][0]["container_format"], "XFIR")
            self.assertEqual(report["containers"][0]["decoded_container_format"], "RIFF")
            self.assertEqual(report["containers"][0]["readable_payload_count"], 1)
            self.assertEqual(report["protected_container_count"], 0)
            self.assertEqual(map_report["status"], "pass")
            self.assertEqual(map_report["containers"][0]["container_format"], "XFIR")
            self.assertEqual(map_report["containers"][0]["decoded_container_format"], "RIFF")
            self.assertTrue((work / "unpacked" / "containers" / "ready" / "0001_png.png").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_blocks_xfir_with_trailing_unverified_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            readable = make_director_mapped_container(
                [("PNG ", make_png(4, 4, fill=(1, 2, 3, 255)))]
            )
            (source / "READY.dcr").write_bytes(make_xfir_wrapped_riff(readable) + b"extra")

            report = extract_readable_assets(source, work, "original_install_root")
            map_report = build_director_resource_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["protected_container_count"], 1)
            self.assertIn(
                "TSUI_EXTRACT_DIRECTOR_XFIR_READER_REQUIRED",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertEqual(map_report["status"], "blocked")
            self.assertIn(
                "TSUI_DIRECTOR_RESOURCE_MAP_XFIR_READER_REQUIRED",
                {diagnostic["code"] for diagnostic in map_report["diagnostics"]},
            )
            self.assertFalse((work / "unpacked" / "containers" / "ready" / "0001_png.png").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_blocks_opaque_xfir_without_readable_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            (source / "READY.dcr").write_bytes(b"XFIR" + b"\x00" * 20)

            report = extract_readable_assets(source, work, "original_install_root")
            map_report = build_director_resource_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["containers"][0]["readable_payload_count"], 0)
            self.assertEqual(report["protected_container_count"], 1)
            self.assertIn(
                "TSUI_EXTRACT_DIRECTOR_XFIR_READER_REQUIRED",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertEqual(map_report["status"], "blocked")
            self.assertIn(
                "TSUI_DIRECTOR_RESOURCE_MAP_XFIR_READER_REQUIRED",
                {diagnostic["code"] for diagnostic in map_report["diagnostics"]},
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_director_cast_map_report_links_key_cas_and_child_resources_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            key_payload = make_director_key_payload(
                [
                    (1, 1024, "CAS*"),
                    (3, 2, "PNG "),
                ]
            )
            cas_payload = make_director_cas_payload([2])
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("KEY*", key_payload),
                        ("CAS*", cas_payload),
                        ("CASt", b"cast member metadata"),
                        ("PNG ", make_png(4, 4, fill=(1, 2, 3, 255))),
                    ]
                )
            )

            report = build_director_cast_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.director_cast_map.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["member_count"], 1)
            member = report["containers"][0]["members"][0]
            self.assertEqual(member["cast_resource_id"], 2)
            self.assertEqual(member["cast_slot"], 0)
            self.assertEqual(member["library_resource_id"], 1024)
            self.assertEqual(member["child_resources"][0]["resource_id"], 3)
            self.assertEqual(member["child_resources"][0]["tag"], "PNG ")
            self.assertIn("sha256:", member["child_resources"][0]["payload_sha256"])
            self.assertNotIn("cast member metadata", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_director_cast_map_report_reads_sanitized_cast_member_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            metadata = {
                "schema": "tsuinosora.director_cast_member_metadata.v1",
                "kind": "background",
                "route_ids": ["classic.main"],
                "command_ids": ["cmd.bg.title"],
                "anchor": {"x": 0, "y": 0},
                "bounds": {"x": 0, "y": 0, "width": 8, "height": 8},
            }
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        (
                            "KEY*",
                            make_director_key_payload(
                                [
                                    (1, 1024, "CAS*"),
                                    (3, 2, "PNG "),
                                ]
                            ),
                        ),
                        ("CAS*", make_director_cas_payload([2])),
                        ("CASt", json.dumps(metadata).encode("utf-8")),
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                    ]
                )
            )

            report = build_director_cast_map_report(source)
            encoded = json.dumps(report, sort_keys=True)
            member = report["containers"][0]["members"][0]

            self.assertEqual(report["status"], "pass")
            self.assertEqual(member.get("kind"), "background")
            self.assertEqual(member.get("route_ids"), ["classic.main"])
            self.assertEqual(member.get("command_ids"), ["cmd.bg.title"])
            self.assertEqual(member.get("anchor"), {"x": 0, "y": 0})
            self.assertEqual(member.get("bounds"), {"x": 0, "y": 0, "width": 8, "height": 8})
            self.assertEqual(
                member.get("cast_metadata_schema"),
                "tsuinosora.director_cast_member_metadata.v1",
            )
            self.assertIn("sha256:", member.get("cast_metadata_sha256", ""))
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_director_cast_map_report_reads_sanitized_character_atlas_parts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            metadata = {
                "schema": "tsuinosora.director_cast_member_metadata.v1",
                "kind": "character_atlas",
                "route_ids": ["classic.main"],
                "command_ids": ["cmd.hero.pose"],
                "parts": [
                    {
                        "part_id": "part.hero.neutral",
                        "pose_id": "pose.hero",
                        "expression_id": "neutral",
                        "anchor": {"x": 16, "y": 64},
                        "crop": {"x": 0, "y": 0, "width": 32, "height": 64},
                        "layer": "character",
                        "mouth_eye_state_compatible": True,
                        "fallback": "nearest_pose",
                    }
                ],
            }
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        (
                            "KEY*",
                            make_director_key_payload(
                                [
                                    (1, 1024, "CAS*"),
                                    (3, 2, "PNG "),
                                ]
                            ),
                        ),
                        ("CAS*", make_director_cas_payload([2])),
                        ("CASt", json.dumps(metadata).encode("utf-8")),
                        ("PNG ", make_png(32, 64, fill=(0, 0, 0, 0), rects=[(0, 0, 31, 63, (40, 80, 120, 255))])),
                    ]
                )
            )

            report = build_director_cast_map_report(source)
            member = report["containers"][0]["members"][0]

            self.assertEqual(report["status"], "pass")
            self.assertEqual(member.get("kind"), "character_atlas")
            self.assertEqual(len(member.get("parts", [])), 1)
            self.assertEqual(member["parts"][0]["part_id"], "part.hero.neutral")
            self.assertEqual(member["parts"][0]["crop"], {"x": 0, "y": 0, "width": 32, "height": 64})

    def test_director_cast_map_report_blocks_invalid_cast_member_layout_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            metadata = {
                "schema": "tsuinosora.director_cast_member_metadata.v1",
                "kind": "background",
                "route_ids": ["classic.main"],
                "command_ids": ["cmd.bg.title"],
                "anchor": {"x": "left", "y": 0},
                "bounds": {"x": 0, "y": 0, "width": -8, "height": 8},
            }
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        (
                            "KEY*",
                            make_director_key_payload(
                                [
                                    (1, 1024, "CAS*"),
                                    (3, 2, "PNG "),
                                ]
                            ),
                        ),
                        ("CAS*", make_director_cas_payload([2])),
                        ("CASt", json.dumps(metadata).encode("utf-8")),
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                    ]
                )
            )

            report = build_director_cast_map_report(source)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_DIRECTOR_CAST_METADATA_ANCHOR_INVALID", codes)
            self.assertIn("TSUI_DIRECTOR_CAST_METADATA_BOUNDS_INVALID", codes)

    def test_director_cast_map_report_blocks_character_atlas_without_parts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            metadata = {
                "schema": "tsuinosora.director_cast_member_metadata.v1",
                "kind": "character_atlas",
                "route_ids": ["classic.main"],
                "command_ids": ["cmd.hero.pose"],
            }
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        (
                            "KEY*",
                            make_director_key_payload(
                                [
                                    (1, 1024, "CAS*"),
                                    (3, 2, "PNG "),
                                ]
                            ),
                        ),
                        ("CAS*", make_director_cas_payload([2])),
                        ("CASt", json.dumps(metadata).encode("utf-8")),
                        ("PNG ", make_png(32, 64, fill=(0, 0, 0, 0), rects=[(0, 0, 31, 63, (40, 80, 120, 255))])),
                    ]
                )
            )

            report = build_director_cast_map_report(source)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_DIRECTOR_CAST_METADATA_ATLAS_PARTS_MISSING",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )

    def test_cast_source_map_report_preserves_director_character_atlas_parts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "containers" / "ready").mkdir(parents=True)
            (root / "containers" / "ready" / "0003_png.png").write_bytes(
                make_png(32, 64, fill=(0, 0, 0, 0), rects=[(0, 0, 31, 63, (40, 80, 120, 255))])
            )
            payload_hash = "sha256:" + hashlib.sha256(
                (root / "containers" / "ready" / "0003_png.png").read_bytes()
            ).hexdigest()
            parts = [
                {
                    "part_id": "part.hero.neutral",
                    "pose_id": "pose.hero",
                    "expression_id": "neutral",
                    "anchor": {"x": 16, "y": 64},
                    "crop": {"x": 0, "y": 0, "width": 32, "height": 64},
                    "layer": "character",
                    "mouth_eye_state_compatible": True,
                    "fallback": "nearest_pose",
                }
            ]
            (root / "containers" / "ready" / "director_cast_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.director_cast_map.v1",
                        "status": "pass",
                        "container_count": 1,
                        "member_count": 1,
                        "containers": [
                            {
                                "relative_path": "READY.dxr",
                                "status": "pass",
                                "member_count": 1,
                                "members": [
                                    {
                                        "member_id": "ready.cast.1024.0",
                                        "cast_resource_id": 2,
                                        "kind": "character_atlas",
                                        "route_ids": ["classic.main"],
                                        "command_ids": ["cmd.hero.pose"],
                                        "parts": parts,
                                        "child_resources": [
                                            {
                                                "resource_id": 3,
                                                "tag": "PNG ",
                                                "payload_sha256": payload_hash,
                                                "coverage_status": "mapped",
                                            }
                                        ],
                                    }
                                ],
                                "diagnostics": [],
                            }
                        ],
                        "diagnostics": [],
                    }
                ),
                encoding="utf-8",
            )

            report = build_cast_source_map_report(root)
            member = report["members"][0]

            self.assertEqual(report["status"], "pass")
            self.assertEqual(member["kind"], "character_atlas")
            self.assertEqual(member.get("parts"), parts)

    def test_director_cast_map_report_blocks_invalid_key_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            invalid_key = b"\x00\x08\x00\x08\x00\x00\x00\x01\x00\x00\x00\x01" + b"\x00" * 8
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("KEY*", invalid_key),
                        ("CAS*", make_director_cas_payload([2])),
                        ("CASt", b"cast member metadata"),
                    ]
                )
            )

            report = build_director_cast_map_report(source)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_DIRECTOR_CAST_KEY_ENTRY_SIZE_INVALID",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )

    def test_director_cast_map_report_blocks_duplicate_cast_member_bindings(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            key_payload = make_director_key_payload(
                [
                    (1, 1024, "CAS*"),
                    (3, 2048, "CAS*"),
                ]
            )
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("KEY*", key_payload),
                        ("CAS*", make_director_cas_payload([2])),
                        ("CASt", b"cast member metadata"),
                        ("CAS*", make_director_cas_payload([2])),
                    ]
                )
            )

            report = build_director_cast_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_DIRECTOR_CAST_DUPLICATE_MEMBER_BINDING",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn("cast member metadata", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_director_lingo_map_report_records_context_names_scripts_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("Lctx", b"\x00\x01\x00\x02"),
                        ("Lnam", b"secretSymbol\x00"),
                        ("Lscr", b"\x01\x02\x03\x04"),
                    ]
                )
            )

            report = build_director_lingo_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.director_lingo_map.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["script_count"], 1)
            self.assertEqual(report["containers"][0]["context_entry_count"], 1)
            self.assertEqual(report["containers"][0]["name_count"], 1)
            self.assertEqual(report["containers"][0]["name_entry_count"], 1)
            context_resource = next(
                resource for resource in report["containers"][0]["resources"] if resource["tag"] == "Lctx"
            )
            self.assertEqual(context_resource["entry_count"], 1)
            self.assertIn("sha256:", context_resource["entry_table_sha256"])
            name_resource = next(
                resource for resource in report["containers"][0]["resources"] if resource["tag"] == "Lnam"
            )
            self.assertEqual(name_resource["entry_count"], 1)
            self.assertIn("sha256:", name_resource["entry_table_sha256"])
            self.assertNotIn("entry_names_sha256", encoded)
            self.assertNotIn("secretSymbol", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_director_lingo_map_report_blocks_unaligned_context_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("Lctx", b"\x00\x01\x02"),
                        ("Lnam", b"secretSymbol\x00"),
                        ("Lscr", b"\x01\x02\x03\x04"),
                    ]
                )
            )

            report = build_director_lingo_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_DIRECTOR_LINGO_CONTEXT_TABLE_UNALIGNED",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn("secretSymbol", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_director_lingo_map_report_blocks_unterminated_name_table(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            source.mkdir()
            (source / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("Lctx", b"\x00\x01\x00\x02"),
                        ("Lnam", b"secretSymbol"),
                        ("Lscr", b"\x01\x02\x03\x04"),
                    ]
                )
            )

            report = build_director_lingo_map_report(source)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_DIRECTOR_LINGO_NAME_TABLE_UNTERMINATED",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn("secretSymbol", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_reads_script_text_chunk_without_report_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            script = "-- astra route: classic.main -> ending.good choices: choice.start\n"
            (source / "READY.dxr").write_bytes(make_riff_container([("Lscr", script.encode("utf-8"))]))

            report = extract_readable_assets(source, work, "original_install_root")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["files"][0]["format_probe"], "script_text")
            self.assertEqual(report["files"][0]["line_count"], 1)
            self.assertTrue((work / "unpacked" / "containers" / "ready" / "0001_lscr.ls").exists())
            self.assertNotIn("classic.main -> ending.good", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_reads_embedded_lscr_text_after_binary_header(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            script = "-- astra route: classic.main -> ending.good choices: choice.start\n"
            payload = b"\x10\xff\x00\x01BINARY-LSCR-HEADER" + script.encode("utf-8")
            (source / "READY.dxr").write_bytes(make_director_mapped_container([("Lscr", payload)]))

            report = extract_readable_assets(source, work, "original_install_root")
            lingo_map = json.loads(
                (
                    work
                    / "unpacked"
                    / "containers"
                    / "ready"
                    / "director_lingo_map.json"
                ).read_text(encoding="utf-8")
            )
            source_map = json.loads(
                (
                    work
                    / "unpacked"
                    / "containers"
                    / "ready"
                    / "director_lingo_source_map.json"
                ).read_text(encoding="utf-8")
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["files"][0]["format_probe"], "script_text")
            self.assertEqual(lingo_map["unsupported_script_count"], 0)
            self.assertEqual(source_map["routes"][0]["choices"], ["choice.start"])
            self.assertGreater(report["files"][0]["payload_inner_offset"], 0)
            self.assertNotIn("classic.main -> ending.good", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_extract_readable_assets_reads_cast_map_metadata_chunk_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "original"
            work = root / "work"
            source.mkdir()
            cast_map = cast_map_payload("containers/ready/0001_png.png")
            (source / "READY.dxr").write_bytes(
                make_riff_container(
                    [
                        ("PNG ", make_png(4, 4, fill=(1, 2, 3, 255))),
                        ("Cmap", json.dumps(cast_map).encode("utf-8")),
                    ]
                )
            )

            report = extract_readable_assets(source, work, "original_install_root")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["files"][1]["format_probe"], "metadata_json")
            self.assertEqual(report["files"][1]["metadata_schema"], "tsuinosora.cast_map.v1")
            self.assertTrue((work / "unpacked" / "containers" / "ready" / "0002_cmap.json").exists())
            self.assertNotIn("member_id", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_asset_analysis_classifies_background_sprite_and_character_atlas(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            (root / "sprite.png").write_bytes(
                make_png(8, 8, fill=(0, 0, 0, 0), rects=[(2, 1, 5, 7, (200, 30, 50, 255))])
            )
            (root / "atlas.png").write_bytes(
                make_png(
                    12,
                    4,
                    fill=(0, 0, 0, 0),
                    rects=[
                        (1, 1, 3, 3, (20, 200, 50, 255)),
                        (8, 1, 10, 3, (20, 200, 50, 255)),
                    ],
                )
            )

            report = analyze_assets(root, reference_report=None)
            by_name = {asset["relative_path"]: asset for asset in report["assets"]}

            self.assertEqual(report["schema"], "tsuinosora.asset_analysis.v1")
            self.assertEqual(by_name["bg.png"]["classification"], "background")
            self.assertEqual(by_name["sprite.png"]["classification"], "character_sprite")
            self.assertEqual(by_name["atlas.png"]["classification"], "character_atlas")
            self.assertEqual(len(by_name["atlas.png"]["parts"]), 2)
            self.assertEqual(report["quarantine"], [])
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["classification_counts"]["character_atlas"], 1)
            self.assertIn("color_distribution", by_name["bg.png"])
            self.assertIn("edge_padding", by_name["sprite.png"])

    def test_route_graph_report_reads_covered_routes_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                                "choices": ["choice.start"],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_route_graph_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.route_graph_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(report["routes"][0]["terminal"], "ending.good")
            self.assertNotIn("choice.start\n", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_route_graph_report_blocks_payload_and_unsafe_symbols(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "../classic.main",
                                "coverage": "covered",
                                "terminal": "ending/good",
                                "choices": ["choice.start", "../choice.bad"],
                                "script_text": "commercial text must not be serialized",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_route_graph_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["route_count"], 0)
            self.assertIn("TSUI_ROUTE_GRAPH_PAYLOAD_FIELD", codes)
            self.assertIn("TSUI_ROUTE_GRAPH_ROUTE_ID_INVALID", codes)
            self.assertIn("TSUI_ROUTE_GRAPH_TERMINAL_INVALID", codes)
            self.assertIn("TSUI_ROUTE_GRAPH_CHOICE_INVALID", codes)
            self.assertNotIn("commercial text must not be serialized", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_route_graph_report_blocks_conflicting_duplicate_route_ids(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                                "choices": ["choice.start"],
                            },
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.bad",
                                "choices": ["choice.other"],
                            },
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_route_graph_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_ROUTE_GRAPH_DUPLICATE_ROUTE_CONFLICT",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_route_graph_report_blocks_duplicate_choices_in_route(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                                "choices": ["choice.start", "choice.start"],
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_route_graph_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_ROUTE_GRAPH_DUPLICATE_CHOICE",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_derives_routes_without_script_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Scripts").mkdir()
            (root / "Scripts" / "main.ls").write_text(
                "-- astra route: classic.main -> ending.good choices: choice.start\n"
                "put \"commercial text omitted\"\n",
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.script_source_map_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["routes"][0]["route_id"], "classic.main")
            self.assertEqual(report["routes"][0]["line"], 1)
            self.assertNotIn("commercial text omitted", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_conflicting_duplicate_route_ids(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Scripts").mkdir()
            (root / "Scripts" / "main.ls").write_text(
                "-- astra route: classic.main -> ending.good choices: choice.start\n"
                "-- astra route: classic.main -> ending.bad choices: choice.other\n",
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_SCRIPT_SOURCE_MAP_DUPLICATE_ROUTE_CONFLICT",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_duplicate_choices_in_route(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Scripts").mkdir()
            (root / "Scripts" / "main.ls").write_text(
                "-- astra route: classic.main -> ending.good choices: choice.start choice.start\n",
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_SCRIPT_SOURCE_MAP_DUPLICATE_CHOICE",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_reads_sanitized_source_map_sidecar(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "maps").mkdir()
            (root / "maps" / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": "sha256:" + ("2" * 64),
                                "line_count": 12,
                                "script_count": 1,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "choices": ["choice.start"],
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 7,
                                "source_hash": "sha256:" + ("2" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.script_source_map_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["reader_count"], 1)
            self.assertEqual(report["readers"][0]["source_map"], "maps/script_source_map.json")
            self.assertEqual(report["readers"][0]["tool_id"], "tonguetwister.lingo-reader")
            self.assertEqual(report["readers"][0]["tool_hash"], "sha256:" + ("1" * 64))
            self.assertEqual(report["routes"][0]["route_id"], "classic.main")
            self.assertEqual(report["routes"][0]["source_map"], "maps/script_source_map.json")
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_route_source_hash_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": "sha256:" + ("2" * 64),
                                "line_count": 12,
                                "script_count": 1,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 7,
                                "source_hash": "sha256:" + ("3" * 64),
                                "coverage": "covered",
                            },
                            {
                                "route_id": "modern.extra",
                                "terminal": "ending.extra",
                                "source": "containers/ready/missing_lingo_map.json",
                                "line": 4,
                                "source_hash": "sha256:" + ("4" * 64),
                                "coverage": "covered",
                            },
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["route_count"], 0)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_ROUTE_HASH_MISMATCH", codes)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_ROUTE_SOURCE_UNDECLARED", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_declared_source_hash_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lingo_dir = root / "containers" / "ready"
            lingo_dir.mkdir(parents=True)
            lingo_map = lingo_dir / "director_lingo_map.json"
            lingo_map.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.director_lingo_map.v1",
                        "status": "pass",
                        "script_count": 0,
                        "unsupported_script_count": 0,
                        "resources": [],
                        "diagnostics": [],
                    },
                    sort_keys=True,
                ),
                encoding="utf-8",
            )
            (root / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": "sha256:" + ("2" * 64),
                                "line_count": 0,
                                "script_count": 0,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 1,
                                "source_hash": "sha256:" + ("2" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["route_count"], 0)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_SOURCE_HASH_MISMATCH", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_route_line_outside_declared_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": "sha256:" + ("2" * 64),
                                "line_count": 3,
                                "script_count": 1,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 4,
                                "source_hash": "sha256:" + ("2" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["route_count"], 0)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_ROUTE_LINE_OUT_OF_RANGE", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_accepts_sidecar_covering_lingo_bytecode(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lingo_dir = root / "containers" / "ready"
            lingo_dir.mkdir(parents=True)
            lingo_map = lingo_dir / "director_lingo_map.json"
            lingo_bytes = json.dumps(
                {
                    "schema": "tsuinosora.director_lingo_map.v1",
                    "status": "pass",
                    "script_count": 1,
                    "unsupported_script_count": 1,
                    "resources": [
                        {
                            "resource_id": 4,
                            "entry_id": "ready.0004",
                            "tag": "Lscr",
                            "size": 16,
                            "payload_sha256": "sha256:" + ("5" * 64),
                            "coverage_status": "mapped",
                            "requires_bytecode_reader": True,
                        }
                    ],
                    "diagnostics": [],
                    "redaction": {"payload": "omitted", "bytecode": "omitted"},
                },
                sort_keys=True,
            ).encode("utf-8")
            lingo_map.write_bytes(lingo_bytes)
            lingo_hash = "sha256:" + hashlib.sha256(lingo_bytes).hexdigest()
            (root / "maps").mkdir()
            (root / "maps" / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": lingo_hash,
                                "line_count": 0,
                                "script_count": 1,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 1,
                                "source_hash": lingo_hash,
                                "script_resource_id": 4,
                                "script_payload_sha256": "sha256:" + ("5" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(report["routes"][0]["script_resource_id"], 4)
            self.assertEqual(report["routes"][0]["script_payload_sha256"], "sha256:" + ("5" * 64))
            self.assertNotIn("TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_UNSUPPORTED", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_lingo_bytecode_sidecar_without_script_resource(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lingo_dir = root / "containers" / "ready"
            lingo_dir.mkdir(parents=True)
            lingo_map = lingo_dir / "director_lingo_map.json"
            lingo_bytes = json.dumps(
                {
                    "schema": "tsuinosora.director_lingo_map.v1",
                    "status": "pass",
                    "script_count": 1,
                    "unsupported_script_count": 1,
                    "resources": [
                        {
                            "resource_id": 4,
                            "entry_id": "ready.0004",
                            "tag": "Lscr",
                            "payload_sha256": "sha256:" + ("5" * 64),
                            "requires_bytecode_reader": True,
                        }
                    ],
                    "diagnostics": [],
                },
                sort_keys=True,
            ).encode("utf-8")
            lingo_map.write_bytes(lingo_bytes)
            lingo_hash = "sha256:" + hashlib.sha256(lingo_bytes).hexdigest()
            (root / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": lingo_hash,
                                "line_count": 0,
                                "script_count": 1,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 1,
                                "source_hash": lingo_hash,
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_SCRIPT_RESOURCE_REQUIRED", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_lingo_bytecode_script_hash_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lingo_dir = root / "containers" / "ready"
            lingo_dir.mkdir(parents=True)
            lingo_map = lingo_dir / "director_lingo_map.json"
            lingo_bytes = json.dumps(
                {
                    "schema": "tsuinosora.director_lingo_map.v1",
                    "status": "pass",
                    "script_count": 1,
                    "unsupported_script_count": 1,
                    "resources": [
                        {
                            "resource_id": 4,
                            "entry_id": "ready.0004",
                            "tag": "Lscr",
                            "payload_sha256": "sha256:" + ("5" * 64),
                            "requires_bytecode_reader": True,
                        }
                    ],
                    "diagnostics": [],
                },
                sort_keys=True,
            ).encode("utf-8")
            lingo_map.write_bytes(lingo_bytes)
            lingo_hash = "sha256:" + hashlib.sha256(lingo_bytes).hexdigest()
            (root / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": lingo_hash,
                                "line_count": 0,
                                "script_count": 1,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 1,
                                "source_hash": lingo_hash,
                                "script_resource_id": 4,
                                "script_payload_sha256": "sha256:" + ("6" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_SCRIPT_HASH_MISMATCH", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_partial_lingo_bytecode_resource_coverage(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lingo_dir = root / "containers" / "ready"
            lingo_dir.mkdir(parents=True)
            lingo_map = lingo_dir / "director_lingo_map.json"
            lingo_bytes = json.dumps(
                {
                    "schema": "tsuinosora.director_lingo_map.v1",
                    "status": "pass",
                    "script_count": 2,
                    "unsupported_script_count": 2,
                    "resources": [
                        {
                            "resource_id": 4,
                            "entry_id": "ready.0004",
                            "tag": "Lscr",
                            "payload_sha256": "sha256:" + ("5" * 64),
                            "requires_bytecode_reader": True,
                        },
                        {
                            "resource_id": 5,
                            "entry_id": "ready.0005",
                            "tag": "Lscr",
                            "payload_sha256": "sha256:" + ("7" * 64),
                            "requires_bytecode_reader": True,
                        },
                    ],
                    "diagnostics": [],
                },
                sort_keys=True,
            ).encode("utf-8")
            lingo_map.write_bytes(lingo_bytes)
            lingo_hash = "sha256:" + hashlib.sha256(lingo_bytes).hexdigest()
            (root / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": lingo_hash,
                                "line_count": 0,
                                "script_count": 2,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 1,
                                "source_hash": lingo_hash,
                                "script_resource_id": 4,
                                "script_payload_sha256": "sha256:" + ("5" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_RESOURCE_UNCOVERED", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_script_source_map_report_blocks_source_map_payload_and_path_leak(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "../local/reader.exe",
                            "tool_hash": "bad-hash",
                            "output_contract": "../route_source_map",
                        },
                        "sources": [{"source": "../source.lscr", "sha256": "bad-hash"}],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "source": "../source.lscr",
                                "line": 1,
                                "script_text": "commercial text must not be serialized",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_script_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_PAYLOAD_FIELD", codes)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_READER_ID_INVALID", codes)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_READER_HASH_INVALID", codes)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_READER_CONTRACT_INVALID", codes)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_SOURCE_INVALID", codes)
            self.assertIn("TSUI_SCRIPT_SOURCE_MAP_HASH_INVALID", codes)
            self.assertNotIn("commercial text must not be serialized", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_cast_source_map_report_maps_members_without_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Assets").mkdir()
            (root / "Assets" / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(root / "cast_map.json", "Assets/bg.png")

            report = build_cast_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.cast_source_map_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["members"][0]["member_id"], "cast.bg.title")
            self.assertEqual(report["members"][0]["source"], "Assets/bg.png")
            self.assertIn("sha256:", report["members"][0]["source_hash"])
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_cast_source_map_report_blocks_payload_fields_in_sidecar(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Assets").mkdir()
            (root / "Assets" / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            cast_map = cast_map_payload("Assets/bg.png")
            cast_map["members"][0]["text"] = "commercial cast text must not be serialized"
            (root / "cast_map.json").write_text(json.dumps(cast_map), encoding="utf-8")

            report = build_cast_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_CAST_SOURCE_MAP_PAYLOAD_FIELD",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn("commercial cast text must not be serialized", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_cast_source_map_report_blocks_declared_source_hash_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Assets").mkdir()
            (root / "Assets" / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            cast_map = cast_map_payload("Assets/bg.png")
            cast_map["members"][0]["source_hash"] = "sha256:" + ("0" * 64)
            (root / "cast_map.json").write_text(json.dumps(cast_map), encoding="utf-8")

            report = build_cast_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_CAST_MEMBER_SOURCE_HASH_MISMATCH", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_cast_source_map_report_blocks_missing_member_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write_cast_map(root / "cast_map.json", "Assets/missing.png")

            report = build_cast_source_map_report(root)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_CAST_MEMBER_SOURCE_MISSING",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )

    def test_cast_source_map_report_blocks_director_child_without_extracted_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "containers" / "ready").mkdir(parents=True)
            (root / "containers" / "ready" / "director_cast_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.director_cast_map.v1",
                        "status": "pass",
                        "container_count": 1,
                        "member_count": 1,
                        "containers": [
                            {
                                "relative_path": "READY.dxr",
                                "status": "pass",
                                "member_count": 1,
                                "members": [
                                    {
                                        "member_id": "ready.cast.1024.0",
                                        "cast_resource_id": 2,
                                        "child_resources": [
                                            {
                                                "resource_id": 3,
                                                "tag": "PNG ",
                                                "payload_sha256": "sha256:" + ("0" * 64),
                                                "coverage_status": "mapped",
                                            }
                                        ],
                                    }
                                ],
                                "diagnostics": [],
                            }
                        ],
                        "diagnostics": [],
                    }
                ),
                encoding="utf-8",
            )

            report = build_cast_source_map_report(root)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_CAST_DIRECTOR_CHILD_SOURCE_MISSING",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )

    def test_cast_source_map_report_blocks_director_cast_payload_fields(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "containers" / "ready").mkdir(parents=True)
            (root / "containers" / "ready" / "0003_png.png").write_bytes(
                make_png(4, 4, fill=(1, 2, 3, 255))
            )
            payload_hash = "sha256:" + hashlib.sha256(
                (root / "containers" / "ready" / "0003_png.png").read_bytes()
            ).hexdigest()
            (root / "containers" / "ready" / "director_cast_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.director_cast_map.v1",
                        "status": "pass",
                        "container_count": 1,
                        "member_count": 1,
                        "payload": "commercial cast payload must not be serialized",
                        "containers": [
                            {
                                "relative_path": "READY.dxr",
                                "status": "pass",
                                "member_count": 1,
                                "members": [
                                    {
                                        "member_id": "ready.cast.1024.0",
                                        "cast_resource_id": 2,
                                        "child_resources": [
                                            {
                                                "resource_id": 3,
                                                "tag": "PNG ",
                                                "payload_sha256": payload_hash,
                                                "coverage_status": "mapped",
                                            }
                                        ],
                                    }
                                ],
                                "diagnostics": [],
                            }
                        ],
                        "diagnostics": [],
                    }
                ),
                encoding="utf-8",
            )

            report = build_cast_source_map_report(root)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_CAST_SOURCE_MAP_PAYLOAD_FIELD",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn("commercial cast payload must not be serialized", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_cast_source_map_report_preserves_director_cast_member_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "containers" / "ready").mkdir(parents=True)
            (root / "containers" / "ready" / "0003_png.png").write_bytes(
                make_png(8, 8, fill=(40, 80, 120, 255))
            )
            payload_hash = "sha256:" + hashlib.sha256(
                (root / "containers" / "ready" / "0003_png.png").read_bytes()
            ).hexdigest()
            (root / "containers" / "ready" / "director_cast_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.director_cast_map.v1",
                        "status": "pass",
                        "container_count": 1,
                        "member_count": 1,
                        "containers": [
                            {
                                "relative_path": "READY.dxr",
                                "status": "pass",
                                "member_count": 1,
                                "members": [
                                    {
                                        "member_id": "ready.cast.1024.0",
                                        "cast_resource_id": 2,
                                        "kind": "background",
                                        "route_ids": ["classic.main"],
                                        "command_ids": ["cmd.bg.title"],
                                        "child_resources": [
                                            {
                                                "resource_id": 3,
                                                "tag": "PNG ",
                                                "payload_sha256": payload_hash,
                                                "coverage_status": "mapped",
                                            }
                                        ],
                                    }
                                ],
                                "diagnostics": [],
                            }
                        ],
                        "diagnostics": [],
                    }
                ),
                encoding="utf-8",
            )

            report = build_cast_source_map_report(root)
            member = report["members"][0]

            self.assertEqual(report["status"], "pass")
            self.assertEqual(member["kind"], "background")
            self.assertEqual(member["route_ids"], ["classic.main"])
            self.assertEqual(member["command_ids"], ["cmd.bg.title"])

    def test_asset_analysis_records_usage_duplicates_and_conflict_quarantine(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Scripts").mkdir()
            (root / "UI").mkdir()
            (root / "bg").mkdir()
            (root / "Scripts" / "main.astra").write_text(
                "\n".join(
                    [
                        "show background dup_a.png",
                        "show text_window UI/text_window.png",
                        "show background bg/hero.png",
                    ]
                ),
                encoding="utf-8",
            )
            duplicate = make_png(8, 8, fill=(40, 80, 120, 255))
            (root / "dup_a.png").write_bytes(duplicate)
            (root / "dup_b.png").write_bytes(duplicate)
            (root / "UI" / "text_window.png").write_bytes(
                make_png(8, 4, fill=(0, 0, 0, 0), rects=[(1, 1, 7, 3, (20, 20, 20, 180))])
            )
            (root / "bg" / "hero.png").write_bytes(
                make_png(8, 8, fill=(0, 0, 0, 0), rects=[(2, 1, 6, 7, (200, 30, 50, 255))])
            )

            report = analyze_assets(root, reference_report=None)
            by_name = {asset["relative_path"]: asset for asset in report["assets"]}

            self.assertEqual(by_name["UI/text_window.png"]["classification"], "text_window")
            self.assertEqual(by_name["dup_a.png"]["duplicate_paths"], ["dup_a.png", "dup_b.png"])
            self.assertEqual(by_name["dup_a.png"]["script_references"][0]["source"], "Scripts/main.astra")
            self.assertEqual(by_name["dup_a.png"]["script_references"][0]["line"], 1)
            self.assertEqual(by_name["dup_a.png"]["use_timing"], "story_route")
            self.assertEqual(by_name["bg/hero.png"]["classification"], "character_sprite")
            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_ASSET_BACKGROUND_AS_CHARACTER",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertIn("duplicate_hashes", report)

    def test_asset_analysis_quarantines_empty_transparent_images(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "empty.png").write_bytes(make_png(8, 8, fill=(0, 0, 0, 0)))

            report = analyze_assets(root, reference_report=None)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["quarantine"][0]["relative_path"], "empty.png")
            self.assertEqual(report["diagnostics"][0]["code"], "TSUI_ASSET_LOW_CONFIDENCE")

    def test_visual_reference_report_records_hash_dimensions_and_no_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))

            report = build_visual_reference_report(title, game)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.visual_reference_report.v1")
            self.assertEqual(report["references"][0]["logical_id"], "title")
            self.assertEqual(report["references"][0]["dimensions"], {"width": 16, "height": 9})
            self.assertIn("sha256:", report["references"][1]["hash"])
            self.assertNotIn("payload", encoded)

    def test_visual_reference_report_blocks_missing_reference_without_path_leak(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            game = root / "Game.png"
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))

            report = build_visual_reference_report(root / "Title.png", game)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_REFERENCE_MISSING", {diag["code"] for diag in report["diagnostics"]})
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_visual_reference_report_blocks_authoritative_hash_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))

            report = build_visual_reference_report(
                title,
                game,
                expected_hashes={"title": "sha256:" + "0" * 64},
                expected_dimensions={"title": {"width": 16, "height": 9}},
            )

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_REFERENCE_HASH_MISMATCH",
                {diag["code"] for diag in report["diagnostics"]},
            )

    def test_visual_screenshot_capture_and_comparison_pass_with_review(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 31, 255))
            )

            capture = build_visual_screenshot_capture_report(work, visual_capture_config("title"))
            comparison = build_visual_comparison_report(work, capture, visual_reviews("title"))
            encoded = json.dumps(comparison, sort_keys=True)

            self.assertEqual(capture["schema"], "tsuinosora.visual_screenshot_capture_report.v1")
            self.assertEqual(capture["status"], "pass")
            self.assertEqual(capture["checkpoints"][0]["original"]["path"], "screenshots/original/title.png")
            self.assertTrue(capture["checkpoints"][0]["original"]["nonblank"])
            self.assertEqual(comparison["schema"], "tsuinosora.visual_comparison_report.v1")
            self.assertEqual(comparison["status"], "pass")
            self.assertEqual(comparison["checkpoints"][0]["regions"][0]["status"], "pass")
            self.assertEqual(comparison["checkpoints"][0]["visual_review"]["status"], "pass")
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))
            self.assertNotIn("commercial text", encoded)

    def test_visual_capture_records_sanitized_windows_automation_intent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 31, 255))
            )
            config = visual_capture_config("title")
            config["capture_automation"] = visual_capture_automation_config(root, "title")

            capture = build_visual_screenshot_capture_report(work, config)
            encoded = json.dumps(capture, sort_keys=True)

            self.assertEqual(capture["status"], "pass")
            self.assertEqual(capture["automation"]["schema"], "tsuinosora.visual_capture_automation_report.v1")
            self.assertTrue(capture["automation"]["configured"])
            self.assertEqual(capture["automation"]["backend"], "windows_sendinput")
            self.assertEqual(capture["automation"]["session_roles"], ["original", "demo"])
            self.assertEqual(capture["automation"]["checkpoint_scripts"][0]["checkpoint_id"], "title")
            self.assertEqual(capture["automation"]["checkpoint_scripts"][0]["step_count"], 3)
            self.assertTrue(capture["automation"]["automation_hash"].startswith("sha256:"))
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))
            self.assertNotIn("private-title", encoded)
            self.assertNotIn("original.exe", encoded)

    def test_visual_capture_runner_writes_screenshots_before_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            config = visual_capture_config("title")
            config["capture_automation"] = visual_capture_automation_config(root, "title")
            runner = FakeVisualCaptureRunner()

            capture = build_visual_screenshot_capture_report(work, config, automation_runner=runner)
            encoded = json.dumps(capture, sort_keys=True)

            self.assertEqual(capture["status"], "pass")
            self.assertEqual(runner.calls, [("windows_sendinput", "title")])
            self.assertTrue((work / "screenshots" / "original" / "title.png").is_file())
            self.assertTrue((work / "screenshots" / "demo" / "title.png").is_file())
            self.assertEqual(capture["automation"]["execution_status"], "pass")
            self.assertEqual(capture["automation"]["captured_checkpoint_count"], 1)
            self.assertEqual(capture["automation"]["screenshot_count"], 2)
            self.assertTrue(capture["checkpoints"][0]["original"]["nonblank"])
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_visual_capture_runner_requires_both_roles_per_required_checkpoint(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 31, 255))
            )
            config = visual_capture_config("title")
            config["capture_automation"] = visual_capture_automation_config(root, "title")

            capture = build_visual_screenshot_capture_report(
                work,
                config,
                automation_runner=OriginalOnlyVisualCaptureRunner(),
            )
            codes = {diagnostic["code"] for diagnostic in capture["diagnostics"]}
            encoded = json.dumps(capture, sort_keys=True)

            self.assertEqual(capture["status"], "blocked")
            self.assertEqual(capture["automation"]["execution_status"], "blocked")
            self.assertIn("TSUI_VISUAL_CAPTURE_AUTOMATION_ROLE_CAPTURE_MISSING", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_visual_capture_launch_environment_merges_private_values_without_report_leak(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 31, 255))
            )
            config = visual_capture_config("title")
            config["capture_automation"] = visual_capture_automation_config(root, "title")
            config["capture_automation"]["sessions"][0]["launch"]["environment"] = {
                "__COMPAT_LAYER": "RunAsInvoker",
                "PRIVATE_ROOT": str(root / "private"),
            }

            merged = _visual_capture_launch_environment(
                {"BASE": "1"},
                config["capture_automation"]["sessions"][0]["launch"],
            )
            capture = build_visual_screenshot_capture_report(work, config)
            encoded = json.dumps(capture, sort_keys=True)

            self.assertEqual(merged["BASE"], "1")
            self.assertEqual(merged["__COMPAT_LAYER"], "RunAsInvoker")
            self.assertIn("PRIVATE_ROOT", merged)
            self.assertNotIn("PRIVATE_ROOT", encoded)
            self.assertNotIn("RunAsInvoker", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_visual_capture_launch_command_resolves_executable_inside_working_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bundle = root / "bundle"
            bundle.mkdir()
            exe = bundle / "AstraPlayer.exe"
            exe.write_bytes(b"fixture")

            command = _resolve_visual_capture_launch_command(
                ["AstraPlayer.exe", "--launch-report"],
                str(bundle),
            )

            self.assertEqual(Path(command[0]), exe.resolve())
            self.assertEqual(command[1:], ["--launch-report"])

    def test_visual_capture_normalizes_letterboxed_capture_to_original_resolution(self):
        width, height = 12, 10
        rgba = bytearray([0, 0, 0, 255] * width * height)
        for y in range(2, 8):
            for x in range(2, 10):
                offset = (y * width + x) * 4
                rgba[offset : offset + 4] = bytes([10, 200, 120, 255])

        normalized = _normalize_visual_capture_image(
            {"width": width, "height": height, "rgba": bytes(rgba)},
            (8, 6),
            "linear",
        )

        self.assertEqual(normalized["width"], 8)
        self.assertEqual(normalized["height"], 6)
        self.assertEqual(normalized["rgba"], bytes([10, 200, 120, 255] * 8 * 6))

    def test_visual_comparison_blocks_missing_review_and_large_region_delta(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(240, 10, 10, 255))
            )

            capture = build_visual_screenshot_capture_report(work, visual_capture_config("title"))
            comparison = build_visual_comparison_report(work, capture, [])
            codes = {diagnostic["code"] for diagnostic in comparison["diagnostics"]}
            encoded = json.dumps(comparison, sort_keys=True)

            self.assertEqual(comparison["status"], "blocked")
            self.assertIn("TSUI_VISUAL_COMPARISON_REVIEW_MISSING", codes)
            self.assertIn("TSUI_VISUAL_COMPARISON_REGION_DIFF", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_visual_comparison_blocks_external_capture_missing_screenshot(self):
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp) / "work"
            capture = {
                "schema": "tsuinosora.visual_screenshot_capture_report.v1",
                "status": "pass",
                "thresholds": {"max_mean_delta": 2.0, "max_changed_ratio": 0.05},
                "checkpoints": [
                    {
                        "checkpoint_id": "title",
                        "route_id": "classic.main",
                        "required": True,
                        "original": {"path": "screenshots/original/missing.png"},
                        "demo": {"path": "screenshots/demo/missing.png"},
                        "regions": [{"region_id": "full_frame", "x": 0, "y": 0, "width": 8, "height": 8}],
                    }
                ],
            }

            comparison = build_visual_comparison_report(work, capture, visual_reviews("title"))
            codes = {diagnostic["code"] for diagnostic in comparison["diagnostics"]}
            encoded = json.dumps(comparison, sort_keys=True)

            self.assertEqual(comparison["status"], "blocked")
            self.assertIn("TSUI_VISUAL_COMPARISON_SCREENSHOT_MISSING", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_visual_capture_blocks_missing_blank_and_unsafe_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(0, 0, 0, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(0, 0, 0, 0))
            )
            config = visual_capture_config("title")
            config["checkpoints"].append(
                {
                    "checkpoint_id": "bad.path",
                    "route_id": "classic.main",
                    "required": True,
                    "original_screenshot": "../outside.png",
                    "demo_screenshot": "screenshots/demo/missing.png",
                    "regions": [{"region_id": "full_frame", "x": 0, "y": 0, "width": 8, "height": 8}],
                }
            )

            report = build_visual_screenshot_capture_report(work, config)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_VISUAL_CAPTURE_BLANK", codes)
            self.assertIn("TSUI_VISUAL_CAPTURE_PATH_INVALID", codes)
            self.assertIn("TSUI_VISUAL_CAPTURE_SCREENSHOT_MISSING", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_visual_capture_blocks_invalid_automation_intent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            work = root / "work"
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 31, 255))
            )
            config = visual_capture_config("title")
            config["capture_automation"] = {
                "schema": "tsuinosora.visual_capture_automation.v1",
                "backend": "windows_sendinput",
                "sessions": [
                    {
                        "role": "original",
                        "launch": {"command": ["original"]},
                        "window_match": {"process_name": "original"},
                    }
                ],
                "input_scripts": [],
            }

            report = build_visual_screenshot_capture_report(work, config)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_VISUAL_CAPTURE_AUTOMATION_SESSION_MISSING", codes)
            self.assertIn("TSUI_VISUAL_CAPTURE_AUTOMATION_INPUT_SCRIPTS_MISSING", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_conversion_report_blocks_quarantine_and_uses_aliases_only(self):
        inventory = {
            "schema": "tsuinosora.source_inventory.v1",
            "root_alias": "original_install_root",
            "file_count": 1,
            "files": [{"relative_path": "empty.png", "sha256": "sha256:asset", "size": 4}],
        }
        asset_analysis = {
            "schema": "tsuinosora.asset_analysis.v1",
            "status": "blocked",
            "assets": [],
            "quarantine": [{"relative_path": "empty.png"}],
            "diagnostics": [{"code": "TSUI_ASSET_LOW_CONFIDENCE"}],
        }

        report = build_conversion_report(
            inventory,
            asset_analysis,
            routes=[{"route_id": "classic.main", "coverage": "covered"}],
        )
        encoded = json.dumps(report, sort_keys=True)

        self.assertEqual(report["schema"], "tsuinosora.conversion_report.v1")
        self.assertEqual(report["status"], "blocked")
        self.assertEqual(report["diagnostics"][0]["code"], "TSUI_CONVERSION_ASSET_QUARANTINE")
        self.assertNotIn(":", report["inputs"]["original_install_root"])
        self.assertNotIn("\\", encoded)

    def test_route_scenario_generation_and_mount_policy_are_sanitized(self):
        scenarios = build_route_scenarios(
            target="tsuinosora-internal-game",
            profile="classic",
            platform="windows",
            routes=[{"route_id": "classic.main", "choices": ["choice.start"], "terminal": "ending.good"}],
        )
        self.assertEqual(scenarios["schema"], "astra.scenario_refs.v1")
        self.assertEqual(scenarios["scenarios"][0]["target"], "tsuinosora-internal-game")
        self.assertEqual(scenarios["scenarios"][0]["assertions"][0]["coverage"]["routes"], ["ending.good"])

        policy = build_mount_policy(
            target="tsuinosora-patch-game",
            aliases={"original": "original_install_root", "remake": "remake_install_root"},
        )
        self.assertEqual(policy["schema"], "tsuinosora.mount_policy.v1")
        self.assertEqual(policy["status"], "pass")

        blocked = build_mount_policy(
            target="tsuinosora-patch-game",
            aliases={"original": "../source"},
        )
        self.assertEqual(blocked["status"], "blocked")
        self.assertEqual(blocked["diagnostics"][0]["code"], "TSUI_MOUNT_ALIAS_PATH_LEAK")

        patch_scenarios = build_route_scenarios(
            target="tsuinosora-patch-game",
            profile="classic",
            platform="windows",
            routes=[
                {
                    "route_id": "classic.main",
                    "choices": ["choice.start"],
                    "terminal": "ending.good",
                    "mount_assets": [
                        {
                            "alias": "original",
                            "path": "native-assets/backgrounds/opening.png",
                            "role": "background",
                            "sha256": "sha256:" + "1" * 64,
                        }
                    ],
                }
            ],
        )
        patch_route = patch_scenarios["scenarios"][0]
        self.assertEqual(patch_route["mount_assets"][0]["route_id"], "classic.main")
        self.assertEqual(patch_route["mount_assets"][0]["role"], "background")

        blocked_asset_role = build_route_scenarios(
            target="tsuinosora-patch-game",
            profile="classic",
            platform="windows",
            routes=[
                {
                    "route_id": "classic.main",
                    "terminal": "ending.good",
                    "mount_assets": [
                        {
                            "alias": "original",
                            "path": "native-assets/backgrounds/opening.png",
                            "role": "not_a_classification",
                            "sha256": "sha256:" + "1" * 64,
                        }
                    ],
                }
            ],
        )
        self.assertEqual(blocked_asset_role["status"], "blocked")
        self.assertEqual(
            blocked_asset_role["diagnostics"][0]["code"],
            "TSUI_ROUTE_MOUNT_ASSET_ROLE_INVALID",
        )

        web_scenarios = build_route_scenarios(
            target="tsuinosora-patch-game",
            profile="classic",
            platform="web",
            routes=[
                {
                    "route_id": "classic.main",
                    "terminal": "ending.good",
                    "mount_assets": [
                        {
                            "alias": "original",
                            "path": "native-assets/backgrounds/opening.png",
                            "role": "background",
                            "sha256": "sha256:" + "1" * 64,
                        }
                    ],
                }
            ],
        )
        self.assertNotIn("mount_assets", web_scenarios["scenarios"][0])

    def test_modern_profile_report_blocks_core_state_changes_and_missing_fallback(self):
        conversion = {
            "schema": "tsuinosora.conversion_report.v1",
            "status": "pass",
            "routes": [{"route_id": "classic.main", "coverage": "covered"}],
        }
        report = build_modern_profile_report(
            conversion,
            features=[
                {
                    "feature_id": "remake_overlay.hero",
                    "feature_kind": "portrait_overlay",
                    "input_hash": "sha256:input",
                    "output_hash": "sha256:output",
                    "fallback_hash": "sha256:fallback",
                    "independent_switch": True,
                    "affects_core_state": False,
                },
                {
                    "feature_id": "bad.route_patch",
                    "feature_kind": "translation_patch",
                    "input_hash": "sha256:bad",
                    "output_hash": "sha256:bad2",
                    "independent_switch": False,
                    "affects_core_state": True,
                },
            ],
        )
        encoded = json.dumps(report, sort_keys=True)

        self.assertEqual(report["schema"], "tsuinosora.modern_profile_report.v1")
        self.assertEqual(report["status"], "blocked")
        self.assertEqual(report["features"][0]["fallback_hash"], "sha256:fallback")
        self.assertIn("TSUI_MODERN_CORE_STATE_CHANGE", {diag["code"] for diag in report["diagnostics"]})
        self.assertIn("TSUI_MODERN_SWITCH_MISSING", {diag["code"] for diag in report["diagnostics"]})
        self.assertIn("TSUI_MODERN_FALLBACK_MISSING", {diag["code"] for diag in report["diagnostics"]})
        self.assertNotIn("\\", encoded)

    def test_stage3_gate_blocks_missing_source_without_path_leak(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))

            report = build_stage3_gate_report(
                original_root=root / "missing-original",
                work_root=root / "work",
                title_png=title,
                game_png=game,
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.stage3_gate_report.v1")
            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_SOURCE_ROOT_MISSING", {diag["code"] for diag in report["diagnostics"]})
            self.assertIn("TSUI_UNPACKED_ROOT_MISSING", {diag["code"] for diag in report["diagnostics"]})
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))
            self.assertTrue((root / "work" / "reports" / "stage3_gate_report.json").exists())

    def test_stage3_gate_runs_extract_preflight_before_asset_analysis(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            (original / "DATA").mkdir(parents=True)
            (original / "Assets").mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (original / "Assets" / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                routes=[{"route_id": "classic.main", "coverage": "covered", "terminal": "ending.good"}],
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["reports"]["extract_report"], "reports/extract_report.json")
            self.assertTrue((work / "reports" / "extract_report.json").exists())
            self.assertTrue((work / "reports" / "asset_analysis.json").exists())
            self.assertIn(
                "TSUI_EXTRACT_DIRECTOR_READER_REQUIRED",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn("TSUI_UNPACKED_ROOT_MISSING", {diagnostic["code"] for diagnostic in report["diagnostics"]})
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_passes_with_readable_riff_container_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_riff_container(
                    [
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                        (
                            "Cmap",
                            json.dumps(cast_map_payload("containers/ready/0001_png.png")).encode("utf-8"),
                        ),
                    ]
                )
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                routes=[{"route_id": "classic.main", "coverage": "covered", "terminal": "ending.good"}],
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)
            extract_report = json.loads((work / "reports" / "extract_report.json").read_text(encoding="utf-8"))

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["reports"]["extract_report"], "reports/extract_report.json")
            self.assertEqual(extract_report["containers"][0]["readable_payload_count"], 2)
            self.assertTrue((work / "reports" / "asset_analysis.json").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_rearranges_assets_after_analysis_before_conversion(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_riff_container(
                    [
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                        (
                            "Cmap",
                            json.dumps(cast_map_payload("containers/ready/0001_png.png")).encode("utf-8"),
                        ),
                    ]
                )
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                routes=[{"route_id": "classic.main", "coverage": "covered", "terminal": "ending.good"}],
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            conversion = json.loads((work / "reports" / "conversion_report.json").read_text(encoding="utf-8"))
            encoded = json.dumps(conversion, sort_keys=True)

            native_asset = work / "native-assets" / "backgrounds" / "containers" / "ready" / "0001_png.png"
            self.assertEqual(report["status"], "pass")
            self.assertTrue(native_asset.exists())
            self.assertEqual(conversion["counts"]["converted_assets"], 1)
            self.assertEqual(conversion["resources"][0]["source"], "containers/ready/0001_png.png")
            self.assertEqual(
                conversion["resources"][0]["native_path"],
                "native-assets/backgrounds/containers/ready/0001_png.png",
            )
            self.assertEqual(conversion["resources"][0]["classification"], "background")
            self.assertEqual(conversion["resources"][0]["source_hash"], conversion["resources"][0]["converted_hash"])
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_derives_routes_from_extracted_route_graph(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_riff_container([("PNG ", make_png(8, 8, fill=(40, 80, 120, 255)))])
            )
            (original / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            write_cast_map(original / "cast_map.json", "containers/ready/0001_png.png")

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["reports"]["route_graph_report"], "reports/route_graph_report.json")
            self.assertTrue((work / "reports" / "route_graph_report.json").exists())
            self.assertTrue(
                (work / "reports" / "scenario_refs.tsuinosora-internal-game.classic.web.json").exists()
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_derives_routes_from_extracted_script_source_map(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            script = "-- astra route: classic.main -> ending.good choices: choice.start\n"
            (original / "READY.dxr").write_bytes(
                make_riff_container(
                    [
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                        ("Lscr", script.encode("utf-8")),
                        (
                            "Cmap",
                            json.dumps(cast_map_payload("containers/ready/0001_png.png")).encode("utf-8"),
                        ),
                    ]
                )
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(
                report["reports"]["script_source_map_report"],
                "reports/script_source_map_report.json",
            )
            self.assertTrue((work / "reports" / "script_source_map_report.json").exists())
            self.assertNotIn("classic.main -> ending.good", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_blocks_bad_route_graph_even_when_script_source_map_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            script = "-- astra route: classic.main -> ending.good choices: choice.start\n"
            (original / "READY.dxr").write_bytes(
                make_riff_container(
                    [
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                        ("Lscr", script.encode("utf-8")),
                        (
                            "Cmap",
                            json.dumps(cast_map_payload("containers/ready/0001_png.png")).encode("utf-8"),
                        ),
                    ]
                )
            )
            (original / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                                "text": "forbidden commercial text",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["reports"]["route_graph_report"], "reports/route_graph_report.json")
            self.assertEqual(report["reports"]["script_source_map_report"], "reports/script_source_map_report.json")
            self.assertIn(
                "TSUI_ROUTE_GRAPH_PAYLOAD_FIELD",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertNotIn("forbidden commercial text", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_prefers_generated_director_lingo_source_map(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            script = "-- astra route: classic.main -> ending.good choices: choice.start\n"
            (original / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                        ("Lctx", b"\x00\x01\x00\x02"),
                        ("Lnam", b"secretSymbol\x00"),
                        ("Lscr", script.encode("utf-8")),
                        (
                            "Cmap",
                            json.dumps(cast_map_payload("containers/ready/0001_png.png")).encode("utf-8"),
                        ),
                    ]
                )
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            script_report = json.loads((work / "reports" / "script_source_map_report.json").read_text(encoding="utf-8"))
            lingo_source_map = (
                work / "unpacked" / "containers" / "ready" / "director_lingo_source_map.json"
            )
            encoded = json.dumps(report, sort_keys=True)
            encoded_script_report = json.dumps(script_report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertTrue(lingo_source_map.exists())
            self.assertEqual(script_report["route_count"], 1)
            self.assertEqual(
                script_report["routes"][0]["source_map"],
                "containers/ready/director_lingo_source_map.json",
            )
            self.assertEqual(
                script_report["routes"][0]["source"],
                "containers/ready/director_lingo_map.json",
            )
            self.assertNotIn("classic.main -> ending.good", encoded)
            self.assertNotIn("classic.main -> ending.good", encoded_script_report)
            self.assertNotIn("secretSymbol", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_derives_routes_from_sanitized_script_source_map_sidecar(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            (original / "DATA").mkdir(parents=True)
            (unpacked / "maps").mkdir(parents=True)
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            (unpacked / "maps" / "script_source_map.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.script_source_map.v1",
                        "reader": {
                            "tool_id": "tonguetwister.lingo-reader",
                            "tool_hash": "sha256:" + ("1" * 64),
                            "output_contract": "route_source_map",
                        },
                        "sources": [
                            {
                                "source": "containers/ready/director_lingo_map.json",
                                "sha256": "sha256:" + ("2" * 64),
                                "script_count": 1,
                            }
                        ],
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "choices": ["choice.start"],
                                "source": "containers/ready/director_lingo_map.json",
                                "line": 7,
                                "source_hash": "sha256:" + ("2" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)
            script_report = json.loads((work / "reports" / "script_source_map_report.json").read_text(encoding="utf-8"))

            self.assertEqual(report["status"], "pass")
            self.assertEqual(script_report["routes"][0]["source_map"], "maps/script_source_map.json")
            self.assertEqual(report["scenario_refs"][0]["route_count"], 1)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_derives_cast_source_map_from_director_key_cas(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            script = "-- astra route: classic.main -> ending.good choices: choice.start\n"
            key_payload = make_director_key_payload(
                [
                    (1, 1024, "CAS*"),
                    (3, 2, "PNG "),
                ]
            )
            cas_payload = make_director_cas_payload([2])
            (original / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("KEY*", key_payload),
                        ("CAS*", cas_payload),
                        ("CASt", b"cast metadata"),
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                        ("Lscr", script.encode("utf-8")),
                    ]
                )
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            cast_report = json.loads((work / "reports" / "cast_source_map_report.json").read_text(encoding="utf-8"))
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(cast_report["status"], "pass")
            self.assertEqual(cast_report["members"][0]["member_id"], "ready.cast.1024.0")
            self.assertEqual(cast_report["members"][0]["container_entry_id"], "ready.0003")
            self.assertEqual(cast_report["members"][0]["director_child_resource_id"], 3)
            self.assertEqual(cast_report["members"][0]["director_child_tag"], "PNG ")
            self.assertIn("sha256:", cast_report["members"][0]["director_child_payload_sha256"])
            self.assertTrue(cast_report["members"][0]["source"].endswith("_png.png"))
            self.assertNotIn("classic.main -> ending.good", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_blocks_lingo_bytecode_without_source_map_reader(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            work = root / "work"
            original.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_director_mapped_container(
                    [
                        ("PNG ", make_png(8, 8, fill=(40, 80, 120, 255))),
                        (
                            "Cmap",
                            json.dumps(cast_map_payload("containers/ready/0001_png.png")).encode("utf-8"),
                        ),
                        ("Lctx", b"\x00\x01\x00\x02"),
                        ("Lnam", b"secretSymbol\x00"),
                        ("Lscr", b"\x01\x02\x03\x04"),
                    ]
                )
            )

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)
            report_codes = [diagnostic["code"] for diagnostic in report["diagnostics"]]
            conversion_report = json.loads((work / "reports" / "conversion_report.json").read_text(encoding="utf-8"))
            conversion_codes = [diagnostic["code"] for diagnostic in conversion_report["diagnostics"]]

            self.assertEqual(report["status"], "blocked")
            self.assertIn(
                "TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_UNSUPPORTED",
                set(report_codes),
            )
            self.assertEqual(report_codes.count("TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_UNSUPPORTED"), 1)
            self.assertEqual(conversion_codes.count("TSUI_SCRIPT_SOURCE_MAP_LINGO_BYTECODE_UNSUPPORTED"), 1)
            self.assertNotIn("secretSymbol", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_stage3_gate_passes_synthetic_source_unpacked_routes_and_features(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            (original / "DATA").mkdir(parents=True)
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")

            report = build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                routes=[{"route_id": "classic.main", "coverage": "covered", "terminal": "ending.good"}],
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["targets"][0]["target"], "tsuinosora-internal-game")
            self.assertTrue((work / "reports" / "asset_analysis.json").exists())
            self.assertTrue((work / "reports" / "conversion_report.json").exists())
            self.assertTrue(
                (work / "reports" / "scenario_refs.tsuinosora-internal-game.classic.web.json").exists()
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_local_gate_writes_stage3_and_nativevn_reports_when_inputs_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            features = [
                {
                    "feature_id": "remake_overlay.hero",
                    "feature_kind": "portrait_overlay",
                    "input_hash": "sha256:input",
                    "output_hash": "sha256:output",
                    "fallback_hash": "sha256:fallback",
                    "independent_switch": True,
                    "affects_core_state": False,
                }
            ]
            (original / "DATA").mkdir(parents=True)
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            (unpacked / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            write_native_story_ir_fixture(work)
            report = run_local_gate(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                modern_features=features,
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.local_gate_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["reports"]["stage3_gate"], "reports/stage3_gate_report.json")
            self.assertEqual(
                report["reports"]["nativevn_package_input"],
                "reports/nativevn_package_input_report.json",
            )
            self.assertTrue((work / "reports" / "local_gate_report.json").exists())
            self.assertTrue((work / "nativevn" / "project.yaml").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_demo_slice_gate_loads_private_config_and_writes_nativevn_without_path_leak(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            config = root / "demo.config.json"
            features = [
                {
                    "feature_id": "remake_overlay.hero",
                    "feature_kind": "portrait_overlay",
                    "input_hash": "sha256:input",
                    "output_hash": "sha256:output",
                    "fallback_hash": "sha256:fallback",
                    "independent_switch": True,
                    "affects_core_state": False,
                }
            ]
            (original / "DATA").mkdir(parents=True)
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            (unpacked / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "choices": ["choice.start"],
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            config.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.demo_slice_config.v1",
                        "original_install_root": str(original),
                        "local_work_root": str(work),
                        "unpacked_root": str(unpacked),
                        "title_png": str(title),
                        "game_png": str(game),
                        "modern_features": features,
                    }
                ),
                encoding="utf-8",
            )

            write_native_story_ir_fixture(work)
            report = run_demo_slice_gate(config)
            encoded = json.dumps(report, sort_keys=True)
            nativevn_report = json.loads(
                (work / "reports" / "nativevn_package_input_report.json").read_text(encoding="utf-8")
            )

            self.assertEqual(report["schema"], "tsuinosora.demo_slice_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["mode"], "demo-slice")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(report["reports"]["local_gate"], "reports/local_gate_report.json")
            self.assertEqual(report["reports"]["nativevn_package_input"], "reports/nativevn_package_input_report.json")
            self.assertEqual(nativevn_report["status"], "pass")
            self.assertTrue((work / "nativevn" / "project.yaml").exists())
            self.assertTrue((work / "nativevn" / "Scripts" / "main.astra").exists())
            self.assertNotIn(str(config).replace("\\", "/"), encoded.replace("\\", "/"))
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_demo_slice_gate_imports_projectorrays_reader_from_private_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            dump = root / "projectorrays-dump"
            tool = root / "tools" / "projectorrays"
            work = root / "work"
            config = root / "demo.config.json"
            (original / "DATA").mkdir(parents=True)
            (original / "Assets").mkdir()
            (original / "Maps").mkdir()
            dump.mkdir()
            tool.parent.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            tool.write_bytes(b"projectorrays fixture")
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (original / "Assets" / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(original / "Maps" / "cast_map.json", "Assets/bg.png")
            (dump / "main.lingo").write_text(
                "-- astra route: classic.main -> ending.good choices: choice.start\n",
                encoding="utf-8",
            )
            config.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.demo_slice_config.v1",
                        "original_install_root": str(original),
                        "local_work_root": str(work),
                        "title_png": str(title),
                        "game_png": str(game),
                        "projectorrays_tool": str(tool),
                        "projectorrays_dump_root": str(dump),
                    }
                ),
                encoding="utf-8",
            )

            write_native_story_ir_fixture(work)
            report = run_demo_slice_gate(config)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["reports"]["projectorrays_reader"], "reports/projectorrays_reader_report.json")
            self.assertEqual(report["route_count"], 1)
            self.assertTrue((work / "reports" / "projectorrays_reader_report.json").exists())
            self.assertTrue((work / "unpacked" / "projectorrays_script_source_map.json").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_demo_slice_gate_uses_projectorrays_converted_assets_without_readable_extract(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            dump = root / "projectorrays-dump"
            tool = root / "tools" / "projectorrays"
            work = root / "work"
            native = work / "native-assets" / "projectorrays" / "data" / "BITD-1.png"
            config = root / "demo.config.json"
            (original / "DATA").mkdir(parents=True)
            dump.mkdir()
            tool.parent.mkdir()
            native.parent.mkdir(parents=True)
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            tool.write_bytes(b"projectorrays fixture")
            (original / "READY.dxr").write_bytes(b"XFIR" + b"\x00" * 20)
            (dump / "BehaviorScript 105 - GO[1321].ls").write_text(
                "put \"commercial route script omitted\"\n",
                encoding="utf-8",
            )
            native.write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            (work / "reports").mkdir(parents=True)
            (work / "reports" / "projectorrays_converted_resources.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.projectorrays_converted_resources.v1",
                        "status": "pass",
                        "resources": [
                            {
                                "source_alias": "data",
                                "source_relative_path": "BITD-1.bin",
                                "source_sha256": "sha256:" + ("1" * 64),
                                "chunk_fourcc": "BITD",
                                "role": "bitmap_or_palette_backed_image",
                                "native_path": "native-assets/projectorrays/data/BITD-1.png",
                                "converted_sha256": sha256_file(native),
                                "byte_size": native.stat().st_size,
                                "conversion_method": "projectorrays_bitd_to_png",
                                "status": "converted",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            config.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.demo_slice_config.v1",
                        "original_install_root": str(original),
                        "local_work_root": str(work),
                        "title_png": str(title),
                        "game_png": str(game),
                        "projectorrays_tool": str(tool),
                        "projectorrays_dump_root": str(dump),
                    }
                ),
                encoding="utf-8",
            )

            write_native_story_ir_fixture(work)
            report = run_demo_slice_gate(config)
            asset_report = json.loads((work / "reports" / "asset_analysis.json").read_text(encoding="utf-8"))
            native_report = json.loads(
                (work / "reports" / "native_asset_rearrange_report.json").read_text(encoding="utf-8")
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(asset_report["status"], "pass")
            self.assertEqual(asset_report["assets"][0]["relative_path"], "native-assets/projectorrays/data/BITD-1.png")
            self.assertEqual(native_report["status"], "pass")
            self.assertEqual(native_report["converted_assets"], 1)
            self.assertNotIn("TSUI_EXTRACT_DIRECTOR_XFIR_READER_REQUIRED", {d["code"] for d in report["diagnostics"]})
            self.assertNotIn("commercial route script omitted", encoded)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_internal_demo_bundle_builds_artifacts_but_blocks_without_live_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, work = self._write_internal_demo_bundle_fixture(tmp)
            runner = FakeAstraRunner(work)

            report = run_internal_demo_bundle(
                config_path=config,
                repo_root=root,
                astra_bin=root / "astra",
                command_runner=runner,
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["schema"], "tsuinosora.internal_demo_bundle_report.v1")
            self.assertEqual(report["status"], "blocked")
            self.assertIn("windows", report["bundles"])
            self.assertIn("web", report["bundles"])
            self.assertTrue(any(file["role"] == "package" for file in report["files"]))
            self.assertTrue(any(diag["code"] == "TSUI_INTERNAL_DEMO_PLAYER_EVIDENCE_REQUIRED" for diag in report["diagnostics"]))
            self.assertTrue(all(cwd == root for phase, _, cwd in runner.calls if phase.startswith("bundle.")))
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))
            self.assertNotIn("validate.player_full_playable", [phase for phase, _, _ in runner.calls])

    def test_internal_demo_bundle_passes_with_matching_live_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, work = self._write_internal_demo_bundle_fixture(tmp)
            live_report = work / "reports" / "live_player_report.json"
            live_report.parent.mkdir(parents=True, exist_ok=True)
            live_report.write_text(
                json.dumps(
                    {
                        "schema": "astra.player_automation_report.v1",
                        "status": "pass",
                        "target": "tsuinosora-internal-game",
                        "profile": "classic",
                        "platform": "windows",
                        "package_hash": "sha256:placeholder",
                        "transcript_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "route_coverage": ["classic.main"],
                        "checks": [{"id": "player.full_playable", "status": "pass"}],
                    }
                ),
                encoding="utf-8",
            )
            runner = FakeAstraRunner(work)

            report = run_internal_demo_bundle(
                config_path=config,
                repo_root=root,
                astra_bin=root / "astra",
                player_automation_report=live_report,
                command_runner=runner,
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["release_report"], "reports/internal_demo_release_report.json")
            self.assertTrue(any(file["role"] == "release_report" for file in report["files"]))
            self.assertIn("validate.player_full_playable", [phase for phase, _, _ in runner.calls])
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_internal_demo_bundle_blocks_when_visual_acceptance_required_without_capture(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, work = self._write_internal_demo_bundle_fixture(tmp)
            value = json.loads(config.read_text(encoding="utf-8"))
            value["require_visual_screenshot_acceptance"] = True
            config.write_text(json.dumps(value), encoding="utf-8")

            report = run_internal_demo_bundle(
                config_path=config,
                repo_root=root,
                astra_bin=root / "astra",
                command_runner=FakeAstraRunner(work),
            )
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["visual_comparison"], "")
            self.assertIn("TSUI_INTERNAL_DEMO_VISUAL_CAPTURE_REQUIRED", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_internal_demo_bundle_requires_automated_visual_capture_intent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, work = self._write_internal_demo_bundle_fixture(tmp)
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 31, 255))
            )
            value = json.loads(config.read_text(encoding="utf-8"))
            value["require_visual_screenshot_acceptance"] = True
            value["visual_capture"] = visual_capture_config("title")
            value["visual_capture"]["visual_reviews"] = visual_reviews("title")
            config.write_text(json.dumps(value), encoding="utf-8")

            report = run_internal_demo_bundle(
                config_path=config,
                repo_root=root,
                astra_bin=root / "astra",
                command_runner=FakeAstraRunner(work),
            )
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["visual_capture"], "reports/visual_screenshot_capture_report.json")
            self.assertIn("TSUI_INTERNAL_DEMO_VISUAL_AUTOMATION_REQUIRED", codes)
            self.assertNotIn("TSUI_INTERNAL_DEMO_PLAYER_EVIDENCE_REQUIRED", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_internal_demo_bundle_blocks_failed_visual_capture_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, work = self._write_internal_demo_bundle_fixture(tmp)
            (work / "screenshots" / "original").mkdir(parents=True)
            (work / "screenshots" / "demo").mkdir(parents=True)
            (work / "screenshots" / "original" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 30, 255))
            )
            (work / "screenshots" / "demo" / "title.png").write_bytes(
                make_png(8, 8, fill=(10, 20, 31, 255))
            )
            value = json.loads(config.read_text(encoding="utf-8"))
            value["require_visual_screenshot_acceptance"] = True
            value["visual_capture"] = visual_capture_config("title")
            value["visual_capture"]["capture_automation"] = visual_capture_automation_config(root, "title")
            value["visual_capture"]["visual_reviews"] = visual_reviews("title")
            config.write_text(json.dumps(value), encoding="utf-8")

            report = run_internal_demo_bundle(
                config_path=config,
                repo_root=root,
                astra_bin=root / "astra",
                command_runner=FakeAstraRunner(work),
                visual_automation_runner=BlockedVisualCaptureRunner(),
            )
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)
            capture = json.loads((work / "reports" / "visual_screenshot_capture_report.json").read_text(encoding="utf-8"))

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(capture["automation"]["execution_status"], "blocked")
            self.assertIn("TSUI_INTERNAL_DEMO_VISUAL_AUTOMATION_BLOCKED", codes)
            self.assertNotIn("TSUI_INTERNAL_DEMO_VISUAL_AUTOMATION_REQUIRED", codes)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_demo_slice_config_blocks_non_string_player_report_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, _ = self._write_internal_demo_bundle_fixture(tmp)
            value = json.loads(config.read_text(encoding="utf-8"))
            value["player_automation_report"] = {"path": "live_player_report.json"}
            config.write_text(json.dumps(value), encoding="utf-8")

            report = run_internal_demo_bundle(config_path=config, repo_root=root, astra_bin=root / "astra")
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertTrue(any(diag["code"] == "TSUI_DEMO_SLICE_CONFIG_PATH_INVALID" for diag in report["diagnostics"]))
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_internal_demo_bundle_blocks_when_full_resource_conversion_required(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, work = self._write_internal_demo_bundle_fixture(tmp)
            full_dump = root / "full-dump"
            full_dump.mkdir()
            (full_dump / "chunk.bin").write_bytes(b"\x82\x00\x82\x00payload")
            value = json.loads(config.read_text(encoding="utf-8"))
            value["require_full_resource_conversion"] = True
            value["projectorrays_full_dump_roots"] = [{"alias": "data", "path": str(full_dump)}]
            config.write_text(json.dumps(value), encoding="utf-8")

            report = run_internal_demo_bundle(
                config_path=config,
                repo_root=root,
                astra_bin=root / "astra",
                command_runner=FakeAstraRunner(work),
            )
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["full_dump"], "reports/projectorrays_full_dump_report.json")
            self.assertTrue(
                any(diag["code"] == "TSUI_INTERNAL_DEMO_FULL_RESOURCE_CONVERSION_BLOCKED" for diag in report["diagnostics"])
            )
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_internal_demo_bundle_accepts_verified_full_resource_conversion_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, config, work = self._write_internal_demo_bundle_fixture(tmp)
            full_dump = root / "full-dump"
            native = work / "native-assets" / "images" / "bitd-1.png"
            full_dump.mkdir()
            source = full_dump / "BITD-1.bin"
            source.write_bytes(b"\x82\x00\x82\x00payload")
            native.parent.mkdir(parents=True)
            native.write_bytes(make_png(4, 4, fill=(10, 20, 30, 255)))
            (work / "reports").mkdir(parents=True, exist_ok=True)
            (work / "reports" / "projectorrays_converted_resources.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.projectorrays_converted_resources.v1",
                        "resources": [
                            {
                                "source_alias": "data",
                                "source_relative_path": "BITD-1.bin",
                                "source_sha256": sha256_file(source),
                                "chunk_fourcc": "BITD",
                                "role": "bitmap_or_palette_backed_image",
                                "native_path": "native-assets/images/bitd-1.png",
                                "converted_sha256": sha256_file(native),
                                "byte_size": native.stat().st_size,
                                "conversion_method": "projectorrays_bitd_to_png",
                                "status": "converted",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            value = json.loads(config.read_text(encoding="utf-8"))
            value["require_full_resource_conversion"] = True
            value["projectorrays_full_dump_roots"] = [{"alias": "data", "path": str(full_dump)}]
            config.write_text(json.dumps(value), encoding="utf-8")

            report = run_internal_demo_bundle(
                config_path=config,
                repo_root=root,
                astra_bin=root / "astra",
                command_runner=FakeAstraRunner(work),
            )
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["full_dump"], "reports/projectorrays_full_dump_report.json")
            self.assertIn("TSUI_INTERNAL_DEMO_PLAYER_EVIDENCE_REQUIRED", codes)
            self.assertNotIn("TSUI_INTERNAL_DEMO_FULL_RESOURCE_CONVERSION_BLOCKED", codes)
            self.assertIn("windows", report["bundles"])
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def _write_internal_demo_bundle_fixture(self, tmp: str) -> tuple[Path, Path, Path]:
        root = Path(tmp)
        title = root / "Title.png"
        game = root / "Game.png"
        original = root / "original"
        unpacked = root / "unpacked"
        work = root / "work"
        config = root / "demo.config.json"
        (original / "DATA").mkdir(parents=True)
        unpacked.mkdir()
        title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
        game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
        (original / "READY.dxr").write_bytes(b"synthetic director container")
        (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
        (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
        write_cast_map(unpacked / "cast_map.json", "bg.png")
        (unpacked / "route_graph.json").write_text(
            json.dumps(
                {
                    "schema": "tsuinosora.route_graph.v1",
                    "routes": [
                        {
                            "route_id": "classic.main",
                            "terminal": "ending.good",
                            "choices": ["choice.start"],
                            "coverage": "covered",
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )
        config.write_text(
            json.dumps(
                {
                    "schema": "tsuinosora.demo_slice_config.v1",
                    "original_install_root": str(original),
                    "local_work_root": str(work),
                        "unpacked_root": str(unpacked),
                        "title_png": str(title),
                        "game_png": str(game),
                        "require_visual_screenshot_acceptance": False,
                        "modern_features": [
                            {
                                "feature_id": "remake_overlay.hero",
                            "feature_kind": "portrait_overlay",
                            "input_hash": "sha256:input",
                            "output_hash": "sha256:output",
                            "fallback_hash": "sha256:fallback",
                            "independent_switch": True,
                            "affects_core_state": False,
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )
        write_native_story_ir_fixture(work)
        return root, config, work

    def test_demo_slice_gate_blocks_explicit_routes_in_private_config(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            config = root / "demo.config.json"
            original.mkdir()
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_riff_container([("PNG ", make_png(8, 8, fill=(40, 80, 120, 255)))])
            )
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            config.write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.demo_slice_config.v1",
                        "original_install_root": str(original),
                        "local_work_root": str(work),
                        "unpacked_root": str(unpacked),
                        "title_png": str(title),
                        "game_png": str(game),
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            report = run_demo_slice_gate(config)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_DEMO_SLICE_ROUTE_EVIDENCE_REQUIRED", codes)
            self.assertEqual(report["reports"]["nativevn_package_input"], "")
            self.assertFalse((work / "nativevn" / "project.yaml").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_local_gate_blocks_explicit_routes_without_report_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            (original / "DATA").mkdir(parents=True)
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            (unpacked / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "coverage": "covered",
                                "terminal": "ending.good",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            write_native_story_ir_fixture(work)
            report = run_local_gate(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                routes=[
                    {
                        "route_id": "classic.main",
                        "coverage": "covered",
                        "terminal": "ending.good",
                    }
                ],
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )

            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}
            self.assertEqual(report["status"], "blocked")
            self.assertIn("TSUI_LOCAL_GATE_ROUTE_EVIDENCE_REQUIRED", codes)
            self.assertEqual(report["reports"]["nativevn_package_input"], "")
            self.assertFalse((work / "nativevn" / "project.yaml").exists())

    def test_local_gate_reports_routes_derived_from_stage3_reports(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            (original / "DATA").mkdir(parents=True)
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            (unpacked / "route_graph.json").write_text(
                json.dumps(
                    {
                        "schema": "tsuinosora.route_graph.v1",
                        "routes": [
                            {
                                "route_id": "classic.main",
                                "terminal": "ending.good",
                                "choices": ["choice.start"],
                                "source": "route_graph.json",
                                "source_hash": "sha256:" + ("9" * 64),
                                "coverage": "covered",
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )

            write_native_story_ir_fixture(work)
            report = run_local_gate(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            nativevn_report = json.loads(
                (work / "reports" / "nativevn_package_input_report.json").read_text(encoding="utf-8")
            )
            conversion = json.loads((work / "reports" / "conversion_report.json").read_text(encoding="utf-8"))
            story = (work / "nativevn" / "Scripts" / "main.astra").read_text(encoding="utf-8")
            input_lines = (work / "nativevn" / "Automation" / "route.good.jsonl").read_text(encoding="utf-8").splitlines()
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(nativevn_report["route_count"], 1)
            self.assertEqual(conversion["routes"][0]["choices"], ["choice.start"])
            self.assertEqual(conversion["routes"][0]["mount_assets"][0]["path"], "native-assets/backgrounds/bg.png")
            self.assertIn("choice.route.good", story)
            self.assertTrue(all(json.loads(line)["schema"] == "astra.user_input_sequence.v1" for line in input_lines))
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_nativevn_package_input_writes_project_sections_and_scenarios(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            (original / "DATA").mkdir(parents=True)
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(b"synthetic director container")
            (original / "DATA" / "SCENE.dxr").write_bytes(b"synthetic scene container")
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            routes = [
                {
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good",
                    "mount_assets": [
                        {
                            "alias": "original",
                            "path": "native-assets/backgrounds/opening.png",
                            "role": "background",
                            "sha256": "sha256:" + "2" * 64,
                        }
                    ],
                }
            ]

            build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                routes=routes,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            write_native_story_ir_fixture(work)
            report = write_nativevn_package_input(work)
            encoded = json.dumps(report, sort_keys=True)
            project = (work / "nativevn" / "project.yaml").read_text(encoding="utf-8")
            story = (work / "nativevn" / "Scripts" / "main.astra").read_text(encoding="utf-8")

            self.assertEqual(report["schema"], "tsuinosora.nativevn_package_input_report.v1")
            self.assertEqual(report["status"], "pass")
            self.assertIn("files", report)
            file_roles = {entry["role"] for entry in report["files"]}
            self.assertIn("project", file_roles)
            self.assertIn("story", file_roles)
            self.assertIn("package_section", file_roles)
            self.assertIn("physical_input_sequence", file_roles)
            self.assertIn("ui_blueprint", file_roles)
            for entry in report["files"]:
                self.assertTrue(entry["path"].startswith("nativevn/"))
                self.assertTrue(entry["sha256"].startswith("sha256:"))
                self.assertGreater(entry["byte_size"], 0)
            self.assertIn("tsuinosora-internal-game", project)
            self.assertIn("original_resolution:", project)
            self.assertIn("width: 800", project)
            self.assertIn("height: 600", project)
            self.assertIn("scale_filter: linear", project)
            self.assertIn("preview_layers:", project)
            self.assertIn("package:/native-assets/projectorrays/data/MENU/chunks/BITD-444.png", project)
            self.assertIn("package:/native-assets/projectorrays/data/MENU/chunks/BITD-449.png", project)
            self.assertIn("asset_roots:", project)
            self.assertIn("native-assets", project)
            self.assertIn("package_sections:", project)
            self.assertIn("tsuinosora.reference_evidence", project)
            self.assertIn("targets: [tsuinosora-patch-game]", project)
            self.assertIn("choice.route.good", story)
            self.assertTrue((work / "nativevn" / "PackageSections" / "asset_analysis.json").exists())
            self.assertTrue((work / "nativevn" / "native-assets" / "backgrounds" / "bg.png.astra-asset.yaml").exists())
            self.assertTrue((work / "nativevn" / "Automation" / "route.good.jsonl").exists())
            self.assertIn("default_profile: modern", project)
            self.assertIn("ui_provider: astra.ui.yakui", project)
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_nativevn_package_input_preserves_route_choices_in_story_and_scenario(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            original.mkdir()
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_riff_container([("PNG ", make_png(8, 8, fill=(40, 80, 120, 255)))])
            )
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            routes = [
                {
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good",
                    "choices": ["choice.start", "choice.confirm"],
                }
            ]

            build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                routes=routes,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )
            payload = native_story_ir_fixture()
            payload["stories"][0]["states"][0]["scenes"][0]["commands"][1]["options"].append(
                {"option_id": "choice.route.confirm", "text": "private confirmation", "target": "ending.good"}
            )
            payload["routes"][0]["choice_ids"].append("choice.route.confirm")
            write_native_story_ir_fixture(work).write_text(json.dumps(payload), encoding="utf-8")
            report = write_nativevn_package_input(work)
            story = (work / "nativevn" / "Scripts" / "main.astra").read_text(encoding="utf-8")

            self.assertEqual(report["status"], "pass")
            self.assertIn("choice.route.good", story)
            self.assertIn("choice.route.confirm", story)

    def test_nativevn_package_input_blocks_duplicate_choices_from_explicit_routes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            original.mkdir()
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_riff_container([("PNG ", make_png(8, 8, fill=(40, 80, 120, 255)))])
            )
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            valid_routes = [
                {
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good",
                    "choices": ["choice.start"],
                }
            ]
            duplicate_choice_routes = [
                {
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good",
                    "choices": ["choice.start", "choice.start"],
                }
            ]

            build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                routes=valid_routes,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )

            report = write_nativevn_package_input(work, duplicate_choice_routes)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["project"], "")
            self.assertEqual(report["story_source_count"], 0)
            self.assertEqual(report["physical_input_sequence_count"], 0)
            self.assertNotIn("project", {entry["role"] for entry in report["files"]})
            self.assertNotIn("story", {entry["role"] for entry in report["files"]})
            self.assertIn(
                "TSUI_NATIVEVN_EXPLICIT_ROUTE_INPUT_RETIRED",
                {diagnostic["code"] for diagnostic in report["diagnostics"]},
            )
            self.assertFalse((work / "nativevn" / "Scripts" / "main.astra").exists())
            self.assertNotIn(tmp.replace("\\", "/"), encoded.replace("\\", "/"))

    def test_nativevn_package_input_blocks_invalid_explicit_route_metadata(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Title.png"
            game = root / "Game.png"
            original = root / "original"
            unpacked = root / "unpacked"
            work = root / "work"
            original.mkdir()
            unpacked.mkdir()
            title.write_bytes(make_png(16, 9, fill=(10, 20, 30, 255)))
            game.write_bytes(make_png(16, 9, fill=(30, 20, 10, 255)))
            (original / "READY.dxr").write_bytes(
                make_riff_container([("PNG ", make_png(8, 8, fill=(40, 80, 120, 255)))])
            )
            (unpacked / "bg.png").write_bytes(make_png(8, 8, fill=(40, 80, 120, 255)))
            write_cast_map(unpacked / "cast_map.json", "bg.png")
            valid_routes = [
                {
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good",
                    "choices": ["choice.start"],
                }
            ]
            invalid_routes = [
                {
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good",
                    "choices": ["choice.start"],
                },
                {
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.bad",
                    "choices": ["choice.start"],
                },
                {
                    "route_id": "classic/main",
                    "coverage": "missing",
                    "terminal": "ending.good",
                    "choices": ["choice.ok", "bad choice"],
                },
            ]

            build_stage3_gate_report(
                original_root=original,
                work_root=work,
                title_png=title,
                game_png=game,
                unpacked_root=unpacked,
                routes=valid_routes,
                modern_features=[
                    {
                        "feature_id": "remake_overlay.hero",
                        "feature_kind": "portrait_overlay",
                        "input_hash": "sha256:input",
                        "output_hash": "sha256:output",
                        "fallback_hash": "sha256:fallback",
                        "independent_switch": True,
                        "affects_core_state": False,
                    }
                ],
            )

            report = write_nativevn_package_input(work, invalid_routes)
            codes = {diagnostic["code"] for diagnostic in report["diagnostics"]}

            self.assertEqual(report["status"], "blocked")
            self.assertEqual(report["project"], "")
            self.assertEqual(report["story_source_count"], 0)
            self.assertEqual(report["physical_input_sequence_count"], 0)
            self.assertIn("TSUI_NATIVEVN_EXPLICIT_ROUTE_INPUT_RETIRED", codes)
            self.assertFalse((work / "nativevn" / "project.yaml").exists())


def make_png(width, height, fill=(0, 0, 0, 0), rects=None):
    import struct
    import zlib

    rects = rects or []
    pixels = [[fill for _ in range(width)] for _ in range(height)]
    for x0, y0, x1, y1, color in rects:
        for y in range(y0, y1):
            for x in range(x0, x1):
                pixels[y][x] = color

    raw = bytearray()
    for row in pixels:
        raw.append(0)
        for r, g, b, a in row:
            raw.extend([r, g, b, a])

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw)))
        + chunk(b"IEND", b"")
    )


def visual_capture_config(checkpoint_id):
    return {
        "schema": "tsuinosora.visual_capture_config.v1",
        "thresholds": {"max_mean_delta": 2.0, "max_changed_ratio": 0.05},
        "checkpoints": [
            {
                "checkpoint_id": checkpoint_id,
                "route_id": "classic.main",
                "required": True,
                "original_screenshot": f"screenshots/original/{checkpoint_id}.png",
                "demo_screenshot": f"screenshots/demo/{checkpoint_id}.png",
                "regions": [
                    {
                        "region_id": "full_frame",
                        "x": 0,
                        "y": 0,
                        "width": 8,
                        "height": 8,
                        "required": True,
                    }
                ],
            }
        ],
    }


def visual_capture_automation_config(root, checkpoint_id):
    return {
        "schema": "tsuinosora.visual_capture_automation.v1",
        "backend": "windows_sendinput",
        "sessions": [
            {
                "role": "original",
                "launch": {
                    "command": [str(Path(root) / "private" / "original.exe")],
                    "working_directory": str(Path(root) / "private"),
                },
                "window_match": {"title_contains": "private-title", "process_name": "original.exe"},
                "startup_timeout_ms": 15000,
            },
            {
                "role": "demo",
                "launch": {
                    "command": [str(Path(root) / "bundle" / "AstraPlayer.exe")],
                    "working_directory": str(Path(root) / "bundle"),
                },
                "window_match": {"title_contains": "private-title", "process_name": "AstraPlayer.exe"},
                "startup_timeout_ms": 15000,
            },
        ],
        "input_scripts": [
            {
                "checkpoint_id": checkpoint_id,
                "steps": [
                    {"kind": "wait", "duration_ms": 50},
                    {"kind": "key", "key": "enter"},
                    {"kind": "capture", "role": "original"},
                ],
            }
        ],
    }


def visual_reviews(checkpoint_id, status="pass"):
    return [
        {
            "checkpoint_id": checkpoint_id,
            "status": status,
            "reviewer": "vision",
            "summary_hash": "sha256:" + ("7" * 64),
        }
    ]


def sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return "sha256:" + digest.hexdigest()


def make_riff_container(chunks):
    import struct

    body = bytearray(b"MV93")
    for chunk_id, payload in chunks:
        chunk_name = chunk_id.encode("ascii")
        if len(chunk_name) != 4:
            raise ValueError("RIFF chunk id must be four ASCII bytes")
        body.extend(chunk_name)
        body.extend(struct.pack("<I", len(payload)))
        body.extend(payload)
        if len(payload) % 2:
            body.append(0)
    return b"RIFF" + struct.pack("<I", len(body)) + bytes(body)


def make_xfir_wrapped_riff(readable_riff):
    import struct

    payload = b"RIFF" + readable_riff[4:]
    return b"XFIR" + struct.pack("<I", len(payload)) + payload


def make_director_mapped_container(resources, dead_chunks=None):
    import struct

    def append_chunk(body, chunk_id, payload):
        chunk_name = chunk_id.encode("ascii")
        if len(chunk_name) != 4:
            raise ValueError("Director chunk id must be four ASCII bytes")
        offset = 8 + len(body)
        body.extend(chunk_name)
        body.extend(struct.pack("<I", len(payload)))
        body.extend(payload)
        if len(payload) % 2:
            body.append(0)
        return offset

    dead_chunks = dead_chunks or []
    body = bytearray(b"MV93")
    imap_payload = bytearray(struct.pack("<III", 1, 0, 0x04C7))
    append_chunk(body, "imap", imap_payload)
    for chunk_id, payload in dead_chunks:
        append_chunk(body, chunk_id, payload)

    mapped = []
    for chunk_id, payload in resources:
        offset = append_chunk(body, chunk_id, payload)
        mapped.append((chunk_id, len(payload), offset))

    mmap_offset = 8 + len(body)
    mmap_payload = bytearray()
    mmap_payload.extend(struct.pack("<HHII", 24, 20, len(mapped), len(mapped)))
    mmap_payload.extend(b"\xff" * 8)
    mmap_payload.extend(struct.pack("<I", 0xFFFFFFFF))
    for chunk_id, size, offset in mapped:
        mmap_payload.extend(chunk_id.encode("ascii"))
        mmap_payload.extend(struct.pack("<I", size))
        mmap_payload.extend(struct.pack("<I", offset))
        mmap_payload.extend(struct.pack("<H", 0))
        mmap_payload.extend(struct.pack("<H", 0))
        mmap_payload.extend(struct.pack("<I", 0xFFFFFFFF))
    append_chunk(body, "mmap", mmap_payload)
    body[16:20] = struct.pack("<I", mmap_offset)
    return b"RIFF" + struct.pack("<I", len(body)) + bytes(body)


def make_director_mapped_container_with_free_entry(resources):
    import struct

    def append_chunk(body, chunk_id, payload):
        chunk_name = chunk_id.encode("ascii")
        if len(chunk_name) != 4:
            raise ValueError("Director chunk id must be four ASCII bytes")
        offset = 8 + len(body)
        body.extend(chunk_name)
        body.extend(struct.pack("<I", len(payload)))
        body.extend(payload)
        if len(payload) % 2:
            body.append(0)
        return offset

    body = bytearray(b"MV93")
    imap_payload = bytearray(struct.pack("<III", 1, 0, 0x04C7))
    append_chunk(body, "imap", imap_payload)

    mapped = []
    for chunk_id, payload in resources:
        offset = append_chunk(body, chunk_id, payload)
        mapped.append((chunk_id, len(payload), offset))

    mmap_offset = 8 + len(body)
    total_entries = len(mapped) + 1
    mmap_payload = bytearray()
    mmap_payload.extend(struct.pack("<HHII", 24, 20, total_entries, total_entries))
    mmap_payload.extend(b"\xff" * 8)
    mmap_payload.extend(struct.pack("<I", len(mapped)))
    for chunk_id, size, offset in mapped:
        mmap_payload.extend(chunk_id.encode("ascii"))
        mmap_payload.extend(struct.pack("<I", size))
        mmap_payload.extend(struct.pack("<I", offset))
        mmap_payload.extend(struct.pack("<H", 0))
        mmap_payload.extend(struct.pack("<H", 0))
        mmap_payload.extend(struct.pack("<I", 0xFFFFFFFF))
    mmap_payload.extend(b"\x00\x00\x00\x00")
    mmap_payload.extend(struct.pack("<I", 0))
    mmap_payload.extend(struct.pack("<I", 0))
    mmap_payload.extend(struct.pack("<H", 0))
    mmap_payload.extend(struct.pack("<H", 0))
    mmap_payload.extend(struct.pack("<I", 0xFFFFFFFF))
    append_chunk(body, "mmap", mmap_payload)
    body[16:20] = struct.pack("<I", mmap_offset)
    return b"RIFF" + struct.pack("<I", len(body)) + bytes(body)


def make_director_key_payload(entries):
    import struct

    payload = bytearray()
    payload.extend(struct.pack("<HHII", 12, 12, len(entries), len(entries)))
    for child_index, parent_index, child_tag in entries:
        payload.extend(struct.pack("<II", child_index, parent_index))
        tag = child_tag.encode("ascii")
        if len(tag) != 4:
            raise ValueError("Director KEY* tag must be four ASCII bytes")
        payload.extend(tag)
    return bytes(payload)


def make_director_cas_payload(cast_resource_ids):
    import struct

    payload = bytearray()
    for resource_id in cast_resource_ids:
        payload.extend(struct.pack(">I", resource_id))
    return bytes(payload)


def cast_map_payload(source):
    return {
        "schema": "tsuinosora.cast_map.v1",
        "members": [
            {
                "member_id": "cast.bg.title",
                "kind": "background",
                "source": source,
                "container_entry_id": "ready.0001",
                "route_ids": ["classic.main"],
                "command_ids": ["cmd.bg.title"],
            }
        ],
    }


def write_cast_map(path, source):
    path.write_text(json.dumps(cast_map_payload(source)), encoding="utf-8")


class FakeAstraRunner:
    def __init__(self, work_root: Path):
        self.work_root = work_root
        self.calls = []

    def __call__(self, phase: str, command: list[str], cwd: Path):
        self.calls.append((phase, list(command), cwd))
        if phase == "cook":
            out = Path(command[command.index("--out") + 1])
            out.mkdir(parents=True, exist_ok=True)
            (out / "cook_manifest.yaml").write_text("schema: astra.cook_manifest.v1\n", encoding="utf-8")
        elif phase == "package":
            out = Path(command[command.index("--out") + 1])
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_bytes(b"synthetic package bytes")
        elif phase.startswith("bundle."):
            out = Path(command[command.index("--out") + 1])
            platform = command[command.index("--platform") + 1]
            out.mkdir(parents=True, exist_ok=True)
            (out / "bundle_manifest.json").write_text(
                json.dumps(
                    {
                        "schema": "astra.standalone_bundle_manifest.v1",
                        "target": "tsuinosora-internal-game",
                        "profile": "classic",
                        "platform": platform,
                        "package": "package/nativevn.astrapkg",
                    }
                ),
                encoding="utf-8",
            )
        elif phase == "validate.player_full_playable":
            package = Path(command[command.index("validate") + 1])
            report = Path(command[command.index("--report") + 1])
            report.parent.mkdir(parents=True, exist_ok=True)
            report.write_text(
                json.dumps(
                    {
                        "schema": "astra.release_report.v1",
                        "package_hash": sha256_file(package),
                        "checks": [{"id": "player.full_playable", "status": "pass"}],
                    }
                ),
                encoding="utf-8",
            )
        return subprocess.CompletedProcess(command, 0, stdout="{}", stderr="")


class FakeVisualCaptureRunner:
    def __init__(self):
        self.calls = []

    def __call__(self, work_root, visual_capture):
        checkpoints = visual_capture.get("checkpoints", [])
        backend = visual_capture.get("capture_automation", {}).get("backend", "")
        checkpoint_id = checkpoints[0]["checkpoint_id"]
        self.calls.append((backend, checkpoint_id))
        original = Path(work_root) / checkpoints[0]["original_screenshot"]
        demo = Path(work_root) / checkpoints[0]["demo_screenshot"]
        original.parent.mkdir(parents=True, exist_ok=True)
        demo.parent.mkdir(parents=True, exist_ok=True)
        original.write_bytes(make_png(8, 8, fill=(10, 20, 30, 255)))
        demo.write_bytes(make_png(8, 8, fill=(10, 20, 31, 255)))
        return {
            "schema": "tsuinosora.visual_capture_automation_execution.v1",
            "status": "pass",
            "captured_checkpoint_count": 1,
            "screenshot_count": 2,
            "captures": [
                {"checkpoint_id": checkpoint_id, "role": "original", "hash": sha256_file(original)},
                {"checkpoint_id": checkpoint_id, "role": "demo", "hash": sha256_file(demo)},
            ],
            "transcript_hash": "sha256:" + ("8" * 64),
            "diagnostics": [],
        }


class OriginalOnlyVisualCaptureRunner:
    def __call__(self, work_root, visual_capture):
        checkpoints = visual_capture.get("checkpoints", [])
        checkpoint_id = checkpoints[0]["checkpoint_id"]
        return {
            "schema": "tsuinosora.visual_capture_automation_execution.v1",
            "status": "pass",
            "captured_checkpoint_count": 1,
            "screenshot_count": 1,
            "captures": [
                {
                    "checkpoint_id": checkpoint_id,
                    "role": "original",
                    "hash": "sha256:" + ("1" * 64),
                }
            ],
            "transcript_hash": "sha256:" + ("8" * 64),
            "diagnostics": [],
        }


class BlockedVisualCaptureRunner:
    def __call__(self, work_root, visual_capture):
        return {
            "schema": "tsuinosora.visual_capture_automation_execution.v1",
            "status": "blocked",
            "captured_checkpoint_count": 0,
            "screenshot_count": 0,
            "transcript_hash": "sha256:" + ("9" * 64),
            "diagnostics": [
                {
                    "code": "TSUI_VISUAL_CAPTURE_AUTOMATION_WINDOW_MISSING",
                    "role": "original",
                    "message": "synthetic visual capture window was unavailable",
                }
            ],
        }


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return f"sha256:{digest}"


if __name__ == "__main__":
    unittest.main()
