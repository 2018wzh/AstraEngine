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
    build_visual_reference_report,
    demo_slice_config_template,
    extract_readable_assets,
    import_projectorrays_reader,
    run_internal_demo_bundle,
    run_demo_slice_gate,
    run_local_gate,
    write_demo_slice_config_template,
    write_nativevn_package_input,
)


class AssetAnalysisTests(unittest.TestCase):
    def test_demo_config_template_uses_repo_relative_private_layout(self):
        template = demo_slice_config_template()
        encoded = json.dumps(template, sort_keys=True)

        self.assertEqual(template["schema"], "tsuinosora.demo_slice_config.v1")
        self.assertEqual(template["local_work_root"], "Examples/TsuiNoSora/.local/work")
        self.assertTrue(template["require_full_resource_conversion"])
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

            report = run_demo_slice_gate(config)
            encoded = json.dumps(report, sort_keys=True)

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["reports"]["projectorrays_reader"], "reports/projectorrays_reader_report.json")
            self.assertEqual(report["route_count"], 1)
            self.assertTrue((work / "reports" / "projectorrays_reader_report.json").exists())
            self.assertTrue((work / "unpacked" / "projectorrays_script_source_map.json").exists())
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
            internal_scenario = json.loads(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-internal-game.classic.headless.classic_main.json"
                ).read_text(encoding="utf-8")
            )
            patch_windows = json.loads(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-patch-game.classic.windows.classic_main.json"
                ).read_text(encoding="utf-8")
            )
            encoded = json.dumps(report, sort_keys=True)
            choose_values = [
                action["player_input"]["value"]
                for action in internal_scenario["actions"]
                if action.get("player_input", {}).get("kind") == "choose"
            ]

            self.assertEqual(report["status"], "pass")
            self.assertEqual(report["route_count"], 1)
            self.assertEqual(nativevn_report["route_count"], 1)
            self.assertEqual(conversion["routes"][0]["choices"], ["choice.start"])
            self.assertEqual(conversion["routes"][0]["mount_assets"][0]["path"], "native-assets/backgrounds/bg.png")
            self.assertIn("option key:choice.start", story)
            self.assertNotIn("option key:choice.classic_main", story)
            self.assertEqual(choose_values, ["choice.start"])
            self.assertEqual(patch_windows["mount_assets"][0]["path"], "native-assets/backgrounds/bg.png")
            self.assertEqual(patch_windows["mount_assets"][0]["role"], "background")
            self.assertTrue(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-internal-game.classic.headless.classic_main.json"
                ).exists()
            )
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
            report = write_nativevn_package_input(work, routes)
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
            self.assertIn("scenario_ref", file_roles)
            for entry in report["files"]:
                self.assertTrue(entry["path"].startswith("nativevn/"))
                self.assertTrue(entry["sha256"].startswith("sha256:"))
                self.assertGreater(entry["byte_size"], 0)
            self.assertIn("tsuinosora-internal-game", project)
            self.assertIn("asset_roots:", project)
            self.assertIn("native-assets", project)
            self.assertIn("package_sections:", project)
            self.assertIn("tsuinosora.reference_evidence", project)
            self.assertIn("targets: [tsuinosora-patch-game]", project)
            self.assertIn("choice.classic_main", story)
            self.assertTrue((work / "nativevn" / "PackageSections" / "asset_analysis.json").exists())
            self.assertTrue((work / "nativevn" / "native-assets" / "backgrounds" / "bg.png.astra-asset.yaml").exists())
            self.assertTrue(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-internal-game.classic.web.classic_main.json"
                ).exists()
            )
            self.assertTrue(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-patch-game.classic.web.classic_main.json"
                ).exists()
            )
            patch_windows = json.loads(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-patch-game.classic.windows.classic_main.json"
                ).read_text(encoding="utf-8")
            )
            patch_web = json.loads(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-patch-game.classic.web.classic_main.json"
                ).read_text(encoding="utf-8")
            )
            self.assertEqual(patch_windows["mount_assets"][0]["route_id"], "classic.main")
            self.assertNotIn("mount_assets", patch_web)
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
            report = write_nativevn_package_input(work, routes)
            story = (work / "nativevn" / "Scripts" / "main.astra").read_text(encoding="utf-8")
            scenario = json.loads(
                (
                    work
                    / "nativevn"
                    / "scenarios"
                    / "tsuinosora-internal-game.classic.headless.classic_main.json"
                ).read_text(encoding="utf-8")
            )
            choose_values = [
                action["player_input"]["value"]
                for action in scenario["actions"]
                if action.get("player_input", {}).get("kind") == "choose"
            ]

            self.assertEqual(report["status"], "pass")
            self.assertIn("option key:choice.start", story)
            self.assertIn("option key:choice.confirm", story)
            self.assertNotIn("option key:choice.classic_main", story)
            self.assertEqual(choose_values, ["choice.start", "choice.confirm"])

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
            self.assertEqual(report["story"], "")
            self.assertEqual(report["scenario_count"], 0)
            self.assertNotIn("project", {entry["role"] for entry in report["files"]})
            self.assertNotIn("story", {entry["role"] for entry in report["files"]})
            self.assertIn(
                "TSUI_NATIVEVN_ROUTE_DUPLICATE_CHOICE",
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
            self.assertEqual(report["story"], "")
            self.assertEqual(report["scenario_count"], 0)
            self.assertIn("TSUI_NATIVEVN_ROUTE_ID_INVALID", codes)
            self.assertIn("TSUI_NATIVEVN_ROUTE_COVERAGE_INVALID", codes)
            self.assertIn("TSUI_NATIVEVN_ROUTE_CHOICE_INVALID", codes)
            self.assertIn("TSUI_NATIVEVN_ROUTE_CONFLICT", codes)
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


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return f"sha256:{digest}"


if __name__ == "__main__":
    unittest.main()
