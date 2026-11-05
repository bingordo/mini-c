#!/usr/bin/env python3
"""S12: measure variadic glibc printf Hello World through GCC and Mini-C.

This is the full S12 glibc stdio printf(%s) compile/link/execute acceptance.
Retain every stage in JSON. Require the GCC reference AND the Mini-C
execution to produce exact stdout, empty stderr, and zero exit status.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "tests/fixtures/glibc-s12/hello_stdio.c"
EXPECTED_SOURCE = (
    '#include <stdio.h>\n\n'
    'int main(void) {\n'
    '    printf("Hello, %s!\\n", "world");\n'
    '    return 0;\n'
    '}\n'
)
EXPECTED_OUTPUT = "Hello, world!\n"
FIRST_MARKER = re.compile(r'^#\s*\d+\s+"([^"]+)"')


def invoke(command: list[str], timeout: int = 35) -> dict:
    try:
        proc = subprocess.run(command, text=True, capture_output=True,
                              timeout=timeout, check=False)
        return {
            "argv": command,
            "exit": proc.returncode,
            "stdout": proc.stdout[-8192:],
            "stderr": proc.stderr[-8192:],
        }
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"argv": command, "error": str(exc)}


def passed(item: dict | None) -> bool:
    return item is not None and item.get("exit") == 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker", type=Path, default=ROOT / "target/debug/mini-c")
    parser.add_argument("--backend", type=Path, default=ROOT / "target/debug/mini-c-cranelift")
    parser.add_argument("--report", type=Path,
                        default=ROOT / "target/s12-glibc-hello-report.json")
    args = parser.parse_args()
    if SOURCE.read_text(encoding="utf-8") != EXPECTED_SOURCE:
        raise RuntimeError("S12 test source changed; review milestone acceptance contract")
    report: dict = {
        "fixture": str(SOURCE.relative_to(ROOT)),
        "system_header": "stdio.h, unmodified from host glibc development package",
        "required_call": "printf(const char *, ...) with one string pointer vararg",
        "abi_requirement": "x86-64 SysV variadic call; AL=0 when no vector registers are used",
        "stages": {},
        "first_preprocessed_markers": [],
    }
    stages = report["stages"]
    with tempfile.TemporaryDirectory(prefix="mini-c-glibc-s12-") as temp:
        base = Path(temp)
        preprocessed = base / "stdio.i"
        gcc_exe = base / "gcc-hello"
        object_file = base / "mini-hello.o"
        mini_exe = base / "mini-hello"

        stages["host_cc_version"] = invoke(["cc", "--version"], timeout=8)
        stages["glibc_version"] = invoke(["getconf", "GNU_LIBC_VERSION"], timeout=8)
        stages["gcc_compile_link"] = invoke([
            "cc", "-std=gnu11", "-D_GNU_SOURCE", str(SOURCE),
            "-o", str(gcc_exe)
        ])
        if passed(stages["gcc_compile_link"]):
            stages["gcc_execute"] = invoke([str(gcc_exe)])
        stages["preprocess"] = invoke([
            "cc", "-E", "-std=gnu11", "-D_GNU_SOURCE",
            str(SOURCE), "-o", str(preprocessed)
        ])
        if passed(stages["preprocess"]) and preprocessed.is_file():
            lines = preprocessed.read_text(encoding="utf-8").splitlines()
            report["preprocessed_lines"] = len(lines)
            report["first_preprocessed_markers"] = [
                line for line in lines if FIRST_MARKER.match(line)
            ][:24]
            stages["mini_c_check"] = invoke([
                str(args.checker), "check", "--cpp=none", str(preprocessed)
            ])
            if not passed(stages["mini_c_check"]):
                diagnostic = stages["mini_c_check"].get("stderr", "")
                # The checker reports offsets in the .i file; preserve the
                # adjacent real glibc text and its nearest #line source marker.
                match = re.search(r":(\d+):(\d+): error:", diagnostic)
                if match:
                    line_no = int(match.group(1))
                    if 1 <= line_no <= len(lines):
                        report["first_error_context"] = {
                            "line": line_no,
                            "column": int(match.group(2)),
                            "origin_marker": next(
                                (line for line in reversed(lines[:line_no])
                                 if FIRST_MARKER.match(line)), None
                            ),
                            "source_lines": [
                                f"{i + 1}: {lines[i]}"
                                for i in range(max(0, line_no - 4),
                                               min(len(lines), line_no + 2))
                            ],
                        }
            if passed(stages["mini_c_check"]):
                stages["mini_c_object"] = invoke([
                    str(args.backend), "--emit-obj", str(object_file),
                    "--cpp=none", str(preprocessed)
                ])
                if passed(stages["mini_c_object"]) and object_file.is_file():
                    stages["mini_c_link"] = invoke([
                        "cc", str(object_file), "-o", str(mini_exe)
                    ])
                    if passed(stages["mini_c_link"]):
                        stages["mini_c_execute"] = invoke([str(mini_exe)])

    report["gcc_reference_ok"] = (
        passed(stages.get("gcc_execute"))
        and stages["gcc_execute"].get("stdout") == EXPECTED_OUTPUT
        and stages["gcc_execute"].get("stderr") == ""
    )
    report["mini_c_hello_ok"] = (
        passed(stages.get("mini_c_execute"))
        and stages["mini_c_execute"].get("stdout") == EXPECTED_OUTPUT
        and stages["mini_c_execute"].get("stderr") == ""
    )
    report["first_blocker"] = next(
        (stage for stage in ("preprocess", "mini_c_check",
                             "mini_c_object", "mini_c_link", "mini_c_execute")
         if not passed(stages.get(stage))), None
    )
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print("S12 glibc stdio Hello World:", "PASS" if report["mini_c_hello_ok"]
          else f"not yet accepted (first blocker: {report['first_blocker']})")
    blocked = report["first_blocker"]
    if blocked and blocked in stages:
        diagnostic = stages[blocked].get("stderr") or stages[blocked].get("error") or ""
        if diagnostic:
            # Bounded log summary; exact full diagnostics remain in JSON.
            print("S12 first blocker diagnostic (max 12 lines):")
            print("\n".join(str(diagnostic).splitlines()[:12]))
    if "first_error_context" in report:
        print("S12 error source context:")
        print("  origin:", report["first_error_context"]["origin_marker"])
        for line in report["first_error_context"]["source_lines"]:
            print(" ", line)
    print("Report:", args.report)
    # S12 P3 acceptance is now mandatory, not a diagnostic-only baseline:
    # the unmodified glibc header must pass Mini-C, compile into an ELF object,
    # link with system libc, and run with exact stdout/stderr and exit status.
    return 0 if report["gcc_reference_ok"] and report["mini_c_hello_ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
