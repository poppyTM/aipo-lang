"""Unit tests for progress synchronization policy."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from progress_delta import LEDGER, implementation_file, requires_ledger


class ProgressPolicyTests(unittest.TestCase):
    def test_language_files_are_tracked(self):
        for source in ("crates/aipo-vm/src/vm/mod.rs", "crates/aipo-cli/Cargo.toml",
                       "packages/foo/main.aipo", "fuzz/fuzz_targets/parser.rs",
                       "examples/demo.aipo", "docs/conformance/programs/hello.stdout"):
            with self.subTest(path=source):
                self.assertTrue(implementation_file(source))

    def test_docs_and_generated_files_are_exempt(self):
        for source in ("website/content/learn/index.md", "crates/README.md",
                       "Cargo.lock", ".github/workflows/example.yml", "docs/conformance/README.md"):
            with self.subTest(path=source):
                self.assertFalse(implementation_file(source))

    def test_requires_ledger_for_code(self):
        self.assertEqual(requires_ledger(["crates/aipo-vm/src/vm/mod.rs"]), (True, ["crates/aipo-vm/src/vm/mod.rs"]))

    def test_accepts_synced_progress(self):
        self.assertFalse(requires_ledger(["crates/aipo-vm/src/vm/mod.rs", LEDGER])[0])

    def test_accepts_document_only(self):
        self.assertFalse(requires_ledger(["website/content/engineering/index.md"])[0])

    def test_rejects_traversal_as_source(self):
        self.assertFalse(implementation_file("../crates/aipo-vm/src/lib.rs"))
        self.assertFalse(implementation_file("/crates/aipo-vm/src/lib.rs"))


if __name__ == "__main__":
    unittest.main()
