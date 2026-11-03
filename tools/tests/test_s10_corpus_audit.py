"""Regression tests for the pinned, unfiltered S10 corpus audit."""
from __future__ import annotations

import argparse
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "mini_c_s10_audit", ROOT / "tools/test_musl_s10.py"
)
s10 = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(s10)


class S10CorpusTests(unittest.TestCase):
    def test_selection_is_fixed_and_unfiltered(self):
        self.assertEqual(len(s10.EXPECTED), 24)
        self.assertEqual(s10.EXPECTED, tuple(sorted(s10.EXPECTED)))
        manifest = s10.validate_manifest()
        self.assertEqual(tuple(manifest["files"]), s10.EXPECTED)
        self.assertEqual(manifest["selection_count"], 24)
        self.assertEqual(manifest["source_form"].split(";")[0],
                         "whole original upstream C files including original includes, macros and declarations")

    def test_changed_or_missing_source_is_fatal(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            shutil.copy(s10.MANIFEST, root / "manifest.json")
            shutil.copy(s10.CORPUS / "UPSTREAM-LICENSE.txt",
                        root / "UPSTREAM-LICENSE.txt")
            directory = root / "string"
            shutil.copytree(s10.SOURCE, directory)
            self.assertEqual(s10.validate_manifest(root / "manifest.json", directory)
                             ["selection_count"], 24)
            sample = directory / "memcmp.c"
            sample.write_bytes(sample.read_bytes() + b"\n/* modified */\n")
            with self.assertRaisesRegex(ValueError, "differs from pinned"):
                s10.validate_manifest(root / "manifest.json", directory)
            shutil.copy(s10.SOURCE / "memcmp.c", sample)
            (directory / "extra.c").write_text("int extra(void) { return 0; }\n")
            with self.assertRaisesRegex(ValueError, "inventory mismatch"):
                s10.validate_manifest(root / "manifest.json", directory)

    def test_all_rejections_counted_even_with_gcc_success(self):
        manifest = s10.validate_manifest()
        args = argparse.Namespace(
            cc="cc", timeout=1, checker=Path("/fake/checker"),
            backend=None, manifest=manifest,
        )
        called = []

        def fake(argv, timeout):
            called.append(argv)
            if "-fsyntax-only" in argv or "-E" in argv:
                return {"exit": 0, "stderr": ""}
            return {"exit": 1, "stderr": "unsupported valid C token"}

        with tempfile.TemporaryDirectory() as td, patch.object(s10, "invoke", side_effect=fake):
            result = s10.check_one("memcmp.c", args, Path(td))
        self.assertTrue(s10.succeeded(result["gcc_syntax"]))
        self.assertEqual(result["stage"], "frontend_rejected")
        self.assertFalse(s10.succeeded(result["frontend"]))
        self.assertEqual(len(called), 3)
        self.assertIsNone(result["native_object"])

    def test_preprocessor_failure_does_not_disappear(self):
        args = argparse.Namespace(
            cc="cc", timeout=1, checker=Path("/fake/checker"),
            backend=None, manifest=s10.validate_manifest(),
        )
        with tempfile.TemporaryDirectory() as td, patch.object(
            s10, "invoke", side_effect=[
                {"exit": 0}, {"exit": 1, "stderr": "missing header"}
            ]
        ) as calls:
            result = s10.check_one("memcmp.c", args, Path(td))
        self.assertEqual(result["stage"], "preprocess_rejected")
        self.assertIsNone(result["frontend"])
        self.assertEqual(calls.call_count, 2)

    def test_stage_separates_accepted_frontend_from_backend(self):
        record = {"preprocess": {"exit": 0}, "frontend": {"exit": 0},
                  "native_object": None}
        self.assertEqual(s10.stage(record, False), "frontend_accepted")
        self.assertEqual(s10.stage(record, True), "object_rejected")
        record["native_object"] = {"exit": 0}
        self.assertEqual(s10.stage(record, True), "object_accepted")

    def test_timeout_is_a_recorded_failure(self):
        with patch.object(s10.subprocess, "run",
                          side_effect=subprocess.TimeoutExpired(["cc"], 1)):
            result = s10.invoke(["cc", "-E", "test.c"], 1)
        self.assertIn("error", result)
        self.assertFalse(s10.succeeded(result))


if __name__ == "__main__":
    unittest.main()
