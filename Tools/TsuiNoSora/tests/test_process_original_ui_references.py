import tempfile
import unittest
from pathlib import Path
from PIL import Image

from Tools.TsuiNoSora.process_original_ui_references import (
    BORDER_CAPTURE_TRANSFORM,
    CANONICAL_RECAPTURE_REFERENCES,
    CLIENT_BORDER_RGBA,
    CLIENT_CAPTURE_CROP,
    CLIENT_CAPTURE_SIZE,
    CROP_BOX,
    DESKTOP_SIZE,
    NATIVE_CAPTURE_SIZE,
    NATIVE_CAPTURE_TRANSFORM,
    OUTPUT_SIZE,
    REFERENCE_INPUTS,
    RECAPTURE_INPUTS,
    STABLE_RECAPTURE_PAIRS,
    ReferenceError,
    normalize_client_capture,
    process_recaptures,
    public_recapture_manifest,
    public_manifest,
    process,
    update_node_map,
    update_reference_manifest,
    validate_content_region,
    validate_desktop_image,
)


class OriginalUiReferenceProcessingTests(unittest.TestCase):
    def test_crop_contract_is_exact_and_resizes_to_800_by_600(self):
        self.assertEqual(CROP_BOX, (1220, 674, 2620, 1724))
        self.assertEqual((CROP_BOX[2] - CROP_BOX[0], CROP_BOX[3] - CROP_BOX[1]), (1400, 1050))
        self.assertEqual(OUTPUT_SIZE, (800, 600))

    def test_mismatched_desktop_capture_is_blocking(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "bad.png"
            image = Image.new("RGB", (3840, 2400), "black")
            image.save(source)
            with self.assertRaisesRegex(ReferenceError, "TSUI_REFERENCE_DESKTOP_SIZE"):
                validate_desktop_image(image, source)

    def test_shifted_content_region_is_blocking(self):
        image = Image.new("RGB", DESKTOP_SIZE, "black")
        for x in range(1218, 1221):
            for y in range(674, 1724):
                image.putpixel((x, y), (255, 255, 255))
        for y in range(671, 674):
            for x in range(1220, 2620):
                image.putpixel((x, y), (255, 255, 255))
        with self.assertRaisesRegex(ReferenceError, "TSUI_REFERENCE_CONTENT_REGION_DRIFT"):
            validate_content_region(image, Path("shifted.png"))

    def test_missing_reference_is_blocking_before_writes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ReferenceError, "TSUI_REFERENCE_INPUT_MISSING"):
                process(root, root / "input", root / "output")

    def test_stable_ids_cover_fourteen_desktop_captures_and_legacy_game(self):
        ids = [reference_id for reference_id, _, _ in REFERENCE_INPUTS]
        self.assertEqual(ids, [f"TSUI1999-UI-{index:03d}" for index in range(1, 15)])

    def test_public_manifest_drops_private_filenames(self):
        result = public_manifest(
            {
                "transform": "fixture",
                "desktop_size": [1, 1],
                "crop_box": [0, 0, 1, 1],
                "output_size": [1, 1],
                "resampler": "fixture",
                "reference_count": 1,
                "references": [{
                    "id": "TSUI1999-UI-001", "role": "fixture", "private_filename": "secret.png",
                    "input_kind": "desktop_crop", "raw_size": [1, 1], "raw_sha256": "a",
                    "crop_box": [0, 0, 1, 1], "crop_sha256": "b", "output_size": [1, 1],
                    "output_sha256": "c"
                }],
            }
        )
        self.assertNotIn("private_filename", result["references"][0])

    def test_client_capture_strips_exact_one_pixel_border_without_resampling(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "capture.png"
            output = root / "normalized.png"
            image = Image.new("RGBA", CLIENT_CAPTURE_SIZE, CLIENT_BORDER_RGBA)
            for x in range(1, 801):
                for y in range(1, 601):
                    image.putpixel((x, y), (x % 255, y % 255, 7, 255))
            image.save(source)
            normalized = normalize_client_capture(source, output)
            self.assertEqual(CLIENT_CAPTURE_CROP, (1, 1, 801, 601))
            self.assertEqual(normalized.size, OUTPUT_SIZE)
            with Image.open(output) as opened:
                self.assertEqual(opened.getpixel((0, 0)), image.getpixel((1, 1)))

    def test_native_client_capture_is_preserved_without_resampling(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "capture.png"
            output = root / "normalized.png"
            image = Image.new("RGBA", NATIVE_CAPTURE_SIZE, (12, 34, 56, 255))
            image.save(source)
            normalized = normalize_client_capture(source, output)
            self.assertEqual(normalized.size, OUTPUT_SIZE)
            self.assertEqual(normalized.tobytes(), image.tobytes())

    def test_client_capture_rejects_wrong_size_or_border(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            wrong_size = root / "wrong-size.png"
            Image.new("RGBA", (799, 600), "black").save(wrong_size)
            with self.assertRaisesRegex(ReferenceError, "TSUI_RECAPTURE_SIZE"):
                normalize_client_capture(wrong_size, root / "unused.png")
            wrong_border = root / "wrong-border.png"
            Image.new("RGBA", CLIENT_CAPTURE_SIZE, "black").save(wrong_border)
            with self.assertRaisesRegex(ReferenceError, "TSUI_RECAPTURE_BORDER"):
                normalize_client_capture(wrong_border, root / "unused.png")
            wrong_mode = root / "wrong-mode.png"
            Image.new("RGB", NATIVE_CAPTURE_SIZE, "black").save(wrong_mode)
            with self.assertRaisesRegex(ReferenceError, "TSUI_RECAPTURE_MODE"):
                normalize_client_capture(wrong_mode, root / "unused.png")

    def test_recapture_pair_drift_is_blocking(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            for _, filename, _ in RECAPTURE_INPUTS:
                Image.new("RGBA", CLIENT_CAPTURE_SIZE, CLIENT_BORDER_RGBA).save(source / filename)
            with Image.open(source / "ui005-00-equation-b.png") as opened:
                image = opened.convert("RGBA")
            image.putpixel((10, 10), (255, 255, 255, 255))
            image.save(source / "ui005-00-equation-b.png")
            with self.assertRaisesRegex(ReferenceError, "TSUI_RECAPTURE_RAW_UNSTABLE"):
                process_recaptures(source, root / "output")

    def test_recapture_pair_png_bytes_must_match(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            for _, filename, _ in RECAPTURE_INPUTS:
                Image.new("RGBA", CLIENT_CAPTURE_SIZE, CLIENT_BORDER_RGBA).save(source / filename)
            target = source / "ui005-00-equation-b.png"
            target.write_bytes(target.read_bytes() + b"metadata-drift")
            with self.assertRaisesRegex(ReferenceError, "TSUI_RECAPTURE_RAW_UNSTABLE"):
                process_recaptures(source, root / "output")

    def test_mixed_recapture_set_produces_thirteen_canonical_references(self):
        native_prefixes = {
            "ui001.",
            "ui006.",
            "ui007.",
            "ui008.",
            "ui010.",
            "ui011.",
            "ui012.",
            "ui013.",
            "ui014.",
        }
        pair_primary_by_capture = {}
        from Tools.TsuiNoSora.process_original_ui_references import STABLE_RECAPTURE_PAIRS

        for primary, stable in STABLE_RECAPTURE_PAIRS:
            pair_primary_by_capture[stable] = primary

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            output = root / "output"
            source.mkdir()
            written: dict[str, bytes] = {}
            for index, (capture_id, filename, _) in enumerate(RECAPTURE_INPUTS):
                primary = pair_primary_by_capture.get(capture_id)
                if primary is not None:
                    (source / filename).write_bytes(written[primary])
                    continue
                is_native = any(capture_id.startswith(prefix) for prefix in native_prefixes)
                size = NATIVE_CAPTURE_SIZE if is_native else CLIENT_CAPTURE_SIZE
                color = (index % 250, (index * 3) % 250, (index * 7) % 250, 255)
                image = Image.new(
                    "RGBA",
                    size,
                    color if is_native else CLIENT_BORDER_RGBA,
                )
                if not is_native:
                    image.paste(color, CLIENT_CAPTURE_CROP)
                image.save(source / filename)
                written[capture_id] = (source / filename).read_bytes()

            manifest = process_recaptures(source, output)

            self.assertEqual(manifest["capture_count"], len(RECAPTURE_INPUTS))
            self.assertEqual(
                manifest["canonical_reference_count"],
                len(CANONICAL_RECAPTURE_REFERENCES),
            )
            self.assertEqual(
                {record["normalization"] for record in manifest["captures"]},
                {NATIVE_CAPTURE_TRANSFORM, BORDER_CAPTURE_TRANSFORM},
            )
            references = sorted((output / "references").glob("*.png"))
            self.assertEqual(len(references), 13)
            for reference in references:
                with Image.open(reference) as opened:
                    self.assertEqual(opened.size, OUTPUT_SIZE)
                    self.assertEqual(opened.mode, "RGBA")

    def test_public_recapture_manifest_drops_private_filenames(self):
        result = public_recapture_manifest(
            {
                "transform": "fixture",
                "accepted_inputs": [
                    {
                        "size": [800, 600],
                        "normalization": NATIVE_CAPTURE_TRANSFORM,
                        "crop_box": None,
                    }
                ],
                "output_size": [800, 600],
                "resampler": "none",
                "capture_count": 1,
                "canonical_reference_count": 0,
                "stable_pairs": [],
                "captures": [{"capture_id": "x", "private_filename": "secret.png"}],
            }
        )
        self.assertNotIn("private_filename", result["captures"][0])

    def test_recapture_updates_public_reference_and_node_evidence(self):
        captures = []
        stable_hashes = {}
        for index, (capture_id, _, _) in enumerate(RECAPTURE_INPUTS):
            output_hash = stable_hashes.get(capture_id, f"{index:064x}")
            captures.append(
                {
                    "capture_id": capture_id,
                    "reference_id": (
                        CANONICAL_RECAPTURE_REFERENCES[capture_id][0]
                        if capture_id in CANONICAL_RECAPTURE_REFERENCES
                        else None
                    ),
                    "normalization": NATIVE_CAPTURE_TRANSFORM,
                    "raw_size": [800, 600],
                    "raw_sha256": output_hash,
                    "crop_box": None,
                    "output_size": [800, 600],
                    "output_sha256": output_hash,
                }
            )
            for primary, stable in STABLE_RECAPTURE_PAIRS:
                if capture_id == primary:
                    stable_hashes[stable] = output_hash
        recapture = {"captures": captures}
        references = [
            {
                "id": f"TSUI1999-UI-{index:03d}",
                "output_sha256": "old",
                "input_kind": "desktop_crop",
            }
            for index in range(1, 16)
        ]
        reference_manifest = {
            "schema": "tsuinosora.original_ui_reference_public_manifest.v1",
            "references": references,
        }
        node_entries = [
            {
                "reference_id": f"TSUI1999-UI-{index:03d}",
                "identity": {"reference_sha256": "sha256:old"},
            }
            for index in range(1, 16)
        ]
        bitmap_identity = node_entries[1]["identity"]
        bitmap_identity["locator"] = {
            "method": "score_bitmap_text",
            "content_sha256": "sha256:" + "f" * 64,
        }
        bitmap_identity["resource_hashes"] = ["sha256:" + "f" * 64]
        node_map = {
            "schema": "tsuinosora.classic_visual_node_map.v3",
            "entries": node_entries,
        }

        updated_references = update_reference_manifest(reference_manifest, recapture)
        updated_nodes = update_node_map(node_map, recapture)

        by_id = {entry["id"]: entry for entry in updated_references["references"]}
        node_by_id = {
            entry["reference_id"]: entry for entry in updated_nodes["entries"]
        }
        self.assertEqual(len(by_id), 15)
        self.assertEqual(len(node_by_id), 15)
        self.assertEqual(by_id["TSUI1999-UI-001"]["input_kind"], "client_identity")
        self.assertNotEqual(by_id["TSUI1999-UI-001"]["output_sha256"], "old")
        self.assertEqual(by_id["TSUI1999-UI-004"]["output_sha256"], "old")
        self.assertEqual(
            node_by_id["TSUI1999-UI-001"]["reference_validation"]["status"],
            "verified",
        )
        self.assertEqual(
            node_by_id["TSUI1999-UI-002"]["reference_validation"]["method"],
            "score_bitmap_resource_closure",
        )
        self.assertEqual(
            node_by_id["TSUI1999-UI-006"]["color_tolerance_approval"]["profile_id"],
            "capture_palette_v1",
        )
        self.assertEqual(
            node_by_id["TSUI1999-UI-004"]["identity"]["reference_sha256"],
            "sha256:old",
        )

    def test_public_evidence_sync_rejects_incomplete_canonical_coverage(self):
        recapture = {"captures": []}
        reference_manifest = {
            "schema": "tsuinosora.original_ui_reference_public_manifest.v1",
            "references": [],
        }
        with self.assertRaisesRegex(ReferenceError, "TSUI_RECAPTURE_CANONICAL_COVERAGE"):
            update_reference_manifest(reference_manifest, recapture)


if __name__ == "__main__":
    unittest.main()
