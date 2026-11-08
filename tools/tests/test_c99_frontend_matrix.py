"""Unit tests for C99 differential accounting without GCC/Clang/Cargo."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import c99_frontend_matrix as matrix


class C99F0Tests(unittest.TestCase):
    def setUp(self):
        self.accept = {"accepted": True}
        self.reject = {"accepted": False}

    def test_seeded_generation_reproducible_and_bounded(self):
        template = "int a[@N@]; int b[@N@];"
        first = matrix.generate(template, 123, 0, "array-size")
        self.assertEqual(first, matrix.generate(template, 123, 0, "array-size"))
        self.assertTrue(first.endswith("\n"))
        self.assertNotIn("@N@", first)
        value = int(first.split("[")[1].split("]")[0])
        self.assertIn(value, range(2, 7))
        self.assertEqual(first.count(str(value)), 2)

    def test_validation_catches_duplicates_and_nonnormative_gaps(self):
        row = {"id": "one", "feature": "F1", "expect": "accept",
               "mini": "supported", "kind": "valid", "test_ref": "test",
               "source": "int f(void){return @N@;}"}
        with self.assertRaisesRegex(ValueError, "duplicate"):
            matrix.validate_manifest({"schema_version": 1, "cases": [row, row]})
        with self.assertRaisesRegex(ValueError, "gap classification"):
            matrix.validate_manifest({"schema_version": 1, "cases": [
                {**row, "expect": "reject", "kind": "constraint", "mini": "gap"}]})

    def test_acceptance_vs_gap_and_reference_contract(self):
        valid = {"expect": "accept", "mini": "supported"}
        gap = {"expect": "accept", "mini": "gap"}
        invalid = {"expect": "reject", "mini": "supported"}
        self.assertEqual(matrix.classify(valid, {
            "gcc": self.accept, "clang": self.accept, "mini_c": self.accept}),
            (True, "matched"))
        self.assertEqual(matrix.classify(valid, {
            "gcc": self.accept, "clang": self.accept, "mini_c": self.reject}),
            (False, "mini-c-differential"))
        self.assertEqual(matrix.classify(gap, {
            "gcc": self.accept, "clang": self.accept, "mini_c": self.reject}),
            (True, "known-gap"))
        self.assertEqual(matrix.classify(gap, {
            "gcc": self.accept, "clang": self.accept, "mini_c": self.accept}),
            (True, "gap-resolved"))
        self.assertEqual(matrix.classify(invalid, {
            "gcc": self.reject, "clang": self.reject, "mini_c": self.reject}),
            (True, "matched"))
        self.assertEqual(matrix.classify(invalid, {
            "gcc": self.accept, "clang": self.reject, "mini_c": self.reject}),
            (False, "reference-disagreement"))
        self.assertEqual(matrix.classify(valid, {
            "gcc": self.accept, "clang": self.accept,
            "mini_c": {"accepted": False, "timed_out": True}}),
            (False, "timeout"))

    def test_invoke_reports_exit_and_timeout(self):
        accepted = matrix.invoke([sys.executable, "-c", "print('ok')"], 3)
        self.assertTrue(accepted["accepted"])
        self.assertEqual(accepted["stdout"].strip(), "ok")
        rejected = matrix.invoke([sys.executable, "-c", "import sys;sys.exit(7)"], 3)
        self.assertFalse(rejected["accepted"])
        self.assertEqual(rejected["exit_code"], 7)
        timeout = matrix.invoke([sys.executable, "-c", "import time;time.sleep(1)"], 0.05)
        self.assertTrue(timeout["timed_out"])

    def test_line_reducer_cannot_reclassify_reference_failures(self):
        # A minimal source-only test for correct reduction stopping:
        # candidates failing the independent oracle cannot qualify.
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "source.c"
            source = "int f(void) {\nreturn 1;\n}\n"
            path.write_text(source)
            reduced = matrix.minimize_lines(
                {"expect": "accept", "mini": "supported"}, source, path,
                "false", "false", "false", 1,
                "mini-c-differential", budget=4)
            self.assertEqual(reduced, source)


if __name__ == "__main__":
    unittest.main()
