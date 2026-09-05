"""Focused regressions for the simplified documentation/inventory commands."""
from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from Tools import check_docs
from Tools import check_headless_test_convergence as test_inventory


class CharterChecksTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / "Docs").mkdir()
        (self.root / "AGENTS.md").write_text("# Working charter\n", encoding="utf-8")

    def document(self, text: str):
        path = self.root / "Docs" / "example.md"
        path.write_text(text, encoding="utf-8")
        return check_docs.check_repository(self.root)[1]

    def test_honest_drafts_and_new_priorities_are_allowed(self):
        self.assertEqual(self.document("TODO: Minori first. Editor is a draft; 未决定。\n"), [])

    def test_links_are_checked_without_enforcing_architecture(self):
        (self.root / "Docs" / "has space.md").write_text("# Existing\n", encoding="utf-8")
        valid = '[ok](has%20space.md#section) [web](https://example.invalid/a)\n'
        self.assertEqual(self.document(valid + '```md\n[example](not-created.md)\n```\n'), [])
        errors = self.document(valid + '[broken](missing.md)\n')
        self.assertEqual(len(errors), 1)
        self.assertIn("missing.md", errors[0])

    def test_malformed_document_is_not_reported_as_valid(self):
        for control in ("\x07", "\x1b", "\x08"):
            with self.subTest(control=repr(control)):
                self.assertTrue(self.document("text" + control))
        (self.root / "Docs" / "example.md").write_bytes(b"\xff")
        self.assertTrue(check_docs.check_repository(self.root)[1])

    def test_missing_charter_is_a_real_error(self):
        (self.root / "AGENTS.md").unlink()
        self.assertTrue(check_docs.check_repository(self.root)[1])

    def test_raw_tests_and_legacy_names_do_not_claim_host_verification(self):
        source = self.root / "Engine" / "Source"
        source.mkdir(parents=True)
        (source / "test.rs").write_text(
            '#[test]\nfn unit() {}\n#[tokio::test(flavor = "current_thread")]\n'
            'async fn asynchronous() {}\n#[astra_headless_test::test]\nfn integration() {}\n'
            '// HeadlessRendererProvider is not banned by a word check.\n',
            encoding="utf-8",
        )
        report = test_inventory.inventory(self.root)
        self.assertEqual(report["status"], "inventory_only")
        self.assertFalse(report["behavior_verified"])
        self.assertEqual(report["inventory"]["ordinary_test_annotations"], 2)
        self.assertEqual(report["inventory"]["headless_test_annotations"], 1)

    def test_missing_sources_do_not_produce_a_fake_inventory(self):
        with self.assertRaises(FileNotFoundError):
            test_inventory.inventory(self.root)


if __name__ == "__main__":
    unittest.main()
