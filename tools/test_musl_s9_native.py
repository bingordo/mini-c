#!/usr/bin/env python3
"""Differentially execute the exact pinned S9 musl functions via Cranelift and GCC.

The Mini-C object contains the unchanged function body from the fixture.
Only the common C test harness is compiled by GCC in the Mini-C executable.
The GCC reference executable compiles the same fixture and harness, using
-fwrapv to define reference signed-overflow behavior (not ISO C semantics).
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

from test_musl_s9 import NAMES

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests/fixtures/musl-s9"
# Exercise C int boundaries, C locale ASCII boundaries, and out-of-range inputs.
VALUES = (
    "-2147483647-1", "-2147483647", "-1000", "-129", "-128", "-1",
    "0", "1", "8", "9", "10", "31", "32", "33", "47", "48",
    "57", "58", "64", "65", "90", "91", "96", "97", "122",
    "123", "126", "127", "128", "129", "255", "256", "1000",
    "2147483647",
)


def validate_snapshot() -> None:
    manifest = json.loads((CORPUS / "manifest.json").read_text())
    if set(manifest["files"]) != {f"{name}.c" for name in NAMES}:
        raise ValueError("S9 manifest does not match pinned twelve-function corpus")
    for filename, pinned in manifest["files"].items():
        data = (CORPUS / filename).read_bytes()
        blob = b"blob " + str(len(data)).encode("ascii") + b"\0" + data
        if hashlib.sha1(blob).hexdigest() != pinned:
            raise ValueError(f"modified pinned S9 source: {filename}")


def harness(name: str) -> str:
    values = ", ".join(VALUES)
    return (
        "#include <stdio.h>\n"
        f"extern int {name}(int);\n"
        "int main(void) {\n"
        f"    int values[] = {{{values}}};\n"
        "    for (unsigned i=0; i<sizeof(values)/sizeof(values[0]); ++i)\n"
        f'        printf("%d\\n", {name}(values[i]));\n'
        "    return 0;\n"
        "}\n"
    )


def run(argv: list[str], timeout: float) -> dict:
    try:
        proc = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
        return {"argv": argv, "returncode": proc.returncode,
                "stdout": proc.stdout, "stderr": proc.stderr}
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"argv": argv, "error": str(exc)}


def differential(name: str, args: argparse.Namespace) -> dict:
    directory = args.failures / name
    directory.mkdir(parents=True, exist_ok=True)
    source = CORPUS / f"{name}.c"
    probe = directory / "harness.c"
    probe.write_text(harness(name))
    obj = directory / "mini.o"
    mini = directory / "mini.exe"
    reference = directory / "gcc.exe"
    common = [args.cc, "-std=c11", "-O2", "-fwrapv", "-fno-builtin"]
    commands = [
        [str(args.backend), "--emit-obj", str(obj), "--cpp=none", str(source)],
        [*common, str(obj), str(probe), "-o", str(mini)],
        [*common, str(source), str(probe), "-o", str(reference)],
        [str(mini)],
        [str(reference)],
    ]
    records = []
    for command in commands:
        result = run(command, args.timeout)
        records.append(result)
        if result.get("returncode") != 0:
            break
    passed = (
        len(records) == 5
        and records[3]["returncode"] == 0
        and records[4]["returncode"] == 0
        and records[3]["stdout"] == records[4]["stdout"]
    )
    report = {"passed": passed, "commands": records}
    if passed:
        shutil.rmtree(directory)
    else:
        (directory / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path, default=Path("target/debug/mini-c-cranelift"))
    parser.add_argument("--cc", default="cc")
    parser.add_argument("--failures", type=Path, default=Path("target/s9-musl-native-failures"))
    parser.add_argument("--report", type=Path, default=Path("target/s9-musl-native-summary.json"))
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    args.backend = args.backend.resolve()
    args.failures = args.failures.resolve()
    if args.timeout <= 0:
        parser.error("timeout must be positive")
    if not args.backend.is_file():
        parser.error(f"missing backend executable: {args.backend}")
    if shutil.which(args.cc) is None:
        parser.error(f"C compiler not available: {args.cc}")
    try:
        validate_snapshot()
    except (OSError, ValueError, KeyError) as exc:
        parser.error(f"invalid pinned corpus: {exc}")
    results = {}
    for name in NAMES:
        result = differential(name, args)
        results[name] = result
        print(f"{'PASS' if result['passed'] else 'FAIL'} {name}")
    count = sum(int(x["passed"]) for x in results.values())
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps({
        "passed": count, "total": len(NAMES), "fixtures": str(CORPUS),
        "results": results,
    }, indent=2) + "\n")
    print(f"Native S9 differential: {count}/{len(NAMES)}; report: {args.report}")
    if count != len(NAMES):
        print(f"Failing reproducers: {args.failures}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
