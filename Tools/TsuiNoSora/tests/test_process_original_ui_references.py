import tempfile
import unittest
from pathlib import Path
from PIL import Image

from Tools.TsuiNoSora.process_original_ui_references import (
    CROP_BOX,
    DESKTOP_SIZE,
    OUTPUT_SIZE,
    REFERENCE_INPUTS,
    ReferenceError,
    public_manifest,
    process,
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


if __name__ == "__main__":
    unittest.main()
