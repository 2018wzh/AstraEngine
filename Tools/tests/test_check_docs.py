from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from Tools.check_docs import check_file


class DocumentationChecks(unittest.TestCase):
    def check_text(self, content):
        with TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "existing file.md").write_text("# Target", encoding="utf-8")
            source = root / "source.md"
            source.write_text(content, encoding="utf-8")
            return check_file(source, root)

    def test_drafts_are_not_architecture_failures(self):
        self.assertEqual([], self.check_text("TODO: planned implementation; no evidence schema."))

    def test_local_links_and_titles(self):
        self.assertEqual([], self.check_text('[file](<existing file.md> "title") [encoded](existing%20file.md#anchor)'))

    def test_missing_inline_and_reference_links(self):
        self.assertEqual(2, len(self.check_text("[a](missing.md)\n[ref]: absent.md")))

    def test_remote_and_internal_links(self):
        self.assertEqual([], self.check_text("[a](https://example.org/a) [b](#heading) [c](mailto:a@example.org)"))

    def test_code_fences_are_not_links(self):
        self.assertEqual([], self.check_text("```md\n[a](missing)\n```\n~~~\n[b](missing)\n~~~"))

    def test_escape_and_control_characters(self):
        self.assertEqual(2, len(self.check_text("[a](../outside)\x07")))

    def test_private_path_redacts_diagnostic(self):
        value = "/home/" + "private-person/source"
        errors = self.check_text(value)
        self.assertEqual(1, len(errors))
        self.assertNotIn(value, errors[0])


if __name__ == "__main__":
    unittest.main()
