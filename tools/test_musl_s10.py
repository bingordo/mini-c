#!/usr/bin/env python3
"""Audit an unfiltered, unchanged real-C corpus against host GCC and Mini-C.

S10 selects the first 24 alphabetically named *.c files from upstream musl's
src/string directory at the fixed manifest commit. Neither source content nor
the selected filenames depend on which examples our compiler can accept.
Each stage is independently accounted for; failures are useful coverage data.

This is a *translation-unit acceptance and optional object-emission* audit.
It does not execute the string functions or establish ABI/runtime correctness.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import re
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests/fixtures/musl-s10"
SOURCE = CORPUS / "string"
MANIFEST = CORPUS / "manifest.json"
EXPECTED = (
    "bcmp.c", "bcopy.c", "bzero.c", "explicit_bzero.c", "index.c",
    "memccpy.c", "memchr.c", "memcmp.c", "memcpy.c", "memmem.c",
    "memmove.c", "mempcpy.c", "memrchr.c", "memset.c", "rindex.c",
    "stpcpy.c", "stpncpy.c", "strcasecmp.c", "strcasestr.c",
    "strcat.c", "strchr.c", "strchrnul.c", "strcmp.c", "strcpy.c",
)


def git_blob(data: bytes) -> str:
    return hashlib.sha1(
        b"blob " + str(len(data)).encode("ascii") + b"\0" + data
    ).hexdigest()


def validate_manifest(manifest_path: Path = MANIFEST,
                      source_dir: Path = SOURCE) -> dict:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    files = manifest["files"]
    if (manifest["selection_count"] != len(EXPECTED)
        or list(files) != list(EXPECTED)
        or manifest["upstream_commit"] !=
            "7813e9119eb5fe681b6750b8a033a0490bca0941"
        or manifest["selection_dir"] != "src/string"
        or manifest["selection_glob"] != "*.c"):
        raise ValueError("S10 manifest selection/provenance does not match pinned 24 files")
    actual = {p.name for p in source_dir.glob("*.c") if p.is_file()}
    if actual != set(EXPECTED):
        raise ValueError(
            f"S10 fixture inventory mismatch: missing {sorted(set(EXPECTED)-actual)}, "
            f"unexpected {sorted(actual-set(EXPECTED))}"
        )
    for name, sha in files.items():
        if git_blob((source_dir / name).read_bytes()) != sha:
            raise ValueError(f"S10 source differs from pinned musl Git blob: {name}")
    if not (manifest_path.parent / "UPSTREAM-LICENSE.txt").is_file():
        raise ValueError("S10 musl upstream license is absent")
    return manifest


def invoke(argv: list[str], timeout: float) -> dict:
    try:
        process = subprocess.run(
            argv, text=True, capture_output=True, timeout=timeout,
            check=False,
        )
        return {
            "exit": process.returncode,
            "stdout": process.stdout[-16000:],
            "stderr": process.stderr[-16000:],
        }
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"error": str(exc)}


def succeeded(record: dict | None) -> bool:
    return record is not None and record.get("exit") == 0


def first_frontend_error(record: dict | None, path: Path) -> dict | None:
    """Identify the exact preprocessed line and nearest #line source marker.

    Distinguishes a host-header blocker from a real upstream function-body
    blocker without rewriting or silently dropping any of the source files.
    """
    if record is None or succeeded(record) or not path.is_file():
        return None
    match = re.search(r":(\d+):(\d+): error:", record.get("stderr", ""))
    if not match:
        return None
    line_number, column = (int(x) for x in match.groups())
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    if line_number < 1 or line_number > len(lines):
        return None
    origin = None
    for line in lines[:line_number]:
        marker = re.match(r'^#\s+\d+\s+"([^"]+)"', line)
        if marker:
            origin = marker.group(1)
    return {
        "preprocessed_line": line_number,
        "column": column,
        "source_marker": origin,
        "line_text": lines[line_number - 1].strip()[:300],
    }


def stage(case: dict, with_backend: bool) -> str:
    if not succeeded(case["preprocess"]):
        return "preprocess_rejected"
    if not succeeded(case["frontend"]):
        return "frontend_rejected"
    if with_backend and not succeeded(case["native_object"]):
        return "object_rejected"
    return "object_accepted" if with_backend else "frontend_accepted"


def check_one(name: str, args: argparse.Namespace, temp: Path) -> dict:
    source = SOURCE / name
    stem = source.stem
    preprocessed = temp / (stem + ".i")
    object_file = temp / (stem + ".o")
    # The GCC baseline is independent of whether the frontend preprocessor
    # or Mini-C accepts the input. Neither stage is used to filter the sample.
    gcc = invoke([
        args.cc, "-std=gnu11", "-D_GNU_SOURCE", "-fsyntax-only", str(source)
    ], args.timeout)
    cpp = invoke([
        args.cc, "-std=gnu11", "-D_GNU_SOURCE", "-E", str(source),
        "-o", str(preprocessed)
    ], args.timeout)
    frontend = None
    native_object = None
    if succeeded(cpp):
        frontend = invoke([
            str(args.checker), "check", "--cpp=none", str(preprocessed)
        ], args.timeout)
        if succeeded(frontend) and args.backend is not None:
            native_object = invoke([
                str(args.backend), "--emit-obj", str(object_file),
                "--cpp=none", str(preprocessed)
            ], args.timeout)
            if succeeded(native_object) and (
                not object_file.is_file() or object_file.stat().st_size == 0
            ):
                native_object = {"error": "backend returned success without an object file"}
    case = {
        "source": f"string/{name}",
        "upstream_blob": args.manifest["files"][name],
        "gcc_syntax": gcc,
        "preprocess": cpp,
        "frontend": frontend,
        "first_frontend_error": first_frontend_error(frontend, preprocessed),
        "native_object": native_object,
    }
    case["stage"] = stage(case, args.backend is not None)
    return case


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker", type=Path,
                        default=Path("target/debug/mini-c"))
    parser.add_argument("--backend", type=Path, default=None,
                        help="optional Mini-C Cranelift object emitter")
    parser.add_argument("--cc", default="cc")
    parser.add_argument("--report", type=Path,
                        default=Path("target/s10-musl-string-acceptance.json"))
    parser.add_argument("--timeout", type=float, default=12)
    parser.add_argument("--min-accepted", type=int, default=0,
                        help="frontend acceptance regression floor (default 0; report all)")
    args = parser.parse_args()
    if args.timeout <= 0 or not 0 <= args.min_accepted <= len(EXPECTED):
        parser.error("timeout must be positive and min-accepted within 0..24")
    try:
        args.manifest = validate_manifest()
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as exc:
        parser.error(f"invalid unmodified S10 corpus: {exc}")
    args.checker = args.checker.resolve()
    if not args.checker.is_file():
        parser.error(f"missing Mini-C checker {args.checker}")
    if args.backend is not None:
        args.backend = args.backend.resolve()
        if not args.backend.is_file():
            parser.error(f"missing Mini-C backend {args.backend}")
    if shutil.which(args.cc) is None:
        parser.error(f"missing host C compiler {args.cc}")
    results = {}
    # One short-lived directory per file prevents stale artifacts and
    # bounds disk use even when host headers expand into large source.
    for name in EXPECTED:
        with TemporaryDirectory(prefix="mini-c-s10-") as td:
            result = check_one(name, args, Path(td))
        results[name] = result
        print(f"{name}: {result['stage']}, "
              f"GCC={'ok' if succeeded(result['gcc_syntax']) else 'reject'}",
              flush=True)
    if len(results) != len(EXPECTED):
        raise AssertionError("S10 must never drop an input file")
    stages = Counter(result["stage"] for result in results.values())
    counts = {
        "total": len(EXPECTED),
        "gcc_syntax": sum(succeeded(r["gcc_syntax"]) for r in results.values()),
        "preprocessed": sum(succeeded(r["preprocess"]) for r in results.values()),
        "frontend_accepted": sum(succeeded(r["frontend"]) for r in results.values()),
        "native_objects": sum(succeeded(r["native_object"]) for r in results.values()),
    }
    report = {
        "corpus": str(SOURCE), "upstream_commit": args.manifest["upstream_commit"],
        "selection": "first 24 alphabetic src/string/*.c at pinned musl commit",
        "source_policy": "complete unmodified upstream C files and includes",
        "system_headers_note": "host system C preprocessor; not musl build environment",
        "check_only": args.backend is None,
        "counts": counts,
        "stages": dict(sorted(stages.items())),
        "results": results,
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"S10: {counts['frontend_accepted']}/{len(EXPECTED)} frontend accepted, "
          f"{counts['native_objects']} native objects, "
          f"{counts['gcc_syntax']} GCC syntax; report: {args.report}")
    if counts["frontend_accepted"] < args.min_accepted:
        print(f"REGRESSION: S10 frontend acceptance below {args.min_accepted}",
              file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
