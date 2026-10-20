"""Unit tests for the reference runner, independent of Cargo and a C compiler."""
import argparse
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import fuzz_reference as runner


class HarnessTests(unittest.TestCase):
    def test_expected_exit_is_one(self):
        self.assertTrue(runner.success({"returncode": 1}, 1))
        self.assertFalse(runner.success({"returncode": 0}, 1))
        self.assertFalse(runner.success({"timeout_seconds": 1}, 1))
        self.assertFalse(runner.success({"returncode": -11}, 1))

    def test_sanitizer_diagnostic_is_not_a_successful_exit_one(self):
        report = {"returncode": 1, "stderr": "runtime error: signed integer overflow", "stdout": ""}
        self.assertTrue(runner.success(report, 1))
        self.assertTrue(runner.sanitizer_reported(report))
        self.assertTrue(runner.sanitizer_reported({
            "returncode": 1, "stderr": "ERROR: AddressSanitizer: heap-buffer-overflow",
            "stdout": "",
        }))
        self.assertFalse(runner.sanitizer_reported({
            "returncode": 1, "stderr": "", "stdout": "",
        }))

    def test_variant_matrix(self):
        gcc = runner.variants("gcc", True)
        clang = runner.variants("/usr/bin/clang-19", True)
        self.assertEqual([name for name, _ in gcc], ["O0", "O2", "sanitized"])
        self.assertIn("-fsanitize=undefined", gcc[-1][1])
        self.assertIn("-fsanitize=address,undefined", clang[-1][1])
        self.assertEqual(len(runner.variants("gcc", False)), 2)

    def test_command_is_argument_vector(self):
        result = runner.invoke([sys.executable, "-c", "import sys; sys.exit(1)"])
        self.assertEqual(result["returncode"], 1)
        self.assertEqual(result["command"][1], "-c")

    def test_failed_generation_keeps_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            failing = root / "invalid-fuzzer"
            failing.write_text("#!/bin/sh\necho intentional-failure >&2\nexit 7\n")
            failing.chmod(0o755)
            args = argparse.Namespace(
                fuzzer=failing, artifacts=root / "artifacts", steps=25,
                compilers=[], skip_sanitizers=True, timeout=4,
            )
            self.assertFalse(runner.check_seed(123, args, {}))
            saved = json.loads(
                (root / "artifacts" / "seed-000123" / "results.json").read_text()
            )
            self.assertEqual(saved["seed"], 123)
            self.assertEqual(saved["steps"], 25)
            self.assertEqual(saved["runs"][0]["returncode"], 7)
            self.assertIn("intentional-failure", saved["runs"][0]["stderr"])


if __name__ == "__main__":
    unittest.main()
