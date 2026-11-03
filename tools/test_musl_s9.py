#!/usr/bin/env python3
"""Measure Mini-C acceptance of the unmodified, pinned S9 musl corpus.

No network access or Cargo invocation. Pass a local checkout of bingordo/compiler.
This is a frontend acceptance gate, not a claim of native-code correctness.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

NAMES = (
    "abs", "isalpha", "isascii", "isblank", "iscntrl", "isdigit",
    "isgraph", "islower", "isprint", "isspace", "isupper", "toascii",
)
RELATIVE = Path("tools/mini-c-driver/examples/benchmark_generated_fusion/musl")


def check_one(checker: Path, source: Path, timeout: float) -> dict:
    command = [str(checker), "check", "--cpp=none", str(source)]
    try:
        proc = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        return {
            "accepted": proc.returncode == 0,
            "exit_code": proc.returncode,
            "stdout": proc.stdout,
            "stderr": proc.stderr,
        }
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"accepted": False, "error": str(exc)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler-repo", type=Path, required=True,
                        help="local bingordo/compiler checkout containing the pinned S9 corpus")
    parser.add_argument("--checker", type=Path, default=Path("target/debug/mini-c"))
    parser.add_argument("--report", type=Path, default=Path("target/s9-musl-acceptance.json"))
    parser.add_argument("--timeout", type=float, default=15)
    parser.add_argument("--min-accepted", type=int, default=1,
                        help="regression floor; raise it after each verified language milestone")
    args = parser.parse_args()

    corpus = args.compiler_repo.resolve() / RELATIVE
    checker = args.checker.resolve()
    if args.timeout <= 0 or not 0 <= args.min_accepted <= len(NAMES):
        parser.error("timeout must be positive and min-accepted within corpus size")
    if not checker.is_file():
        parser.error(f"build mini-c first: missing checker {checker}")
    missing = [name for name in NAMES if not (corpus / f"{name}.c").is_file()]
    if missing:
        parser.error(f"missing pinned S9 fixtures in {corpus}: {', '.join(missing)}")

    results = {}
    for name in NAMES:
        result = check_one(checker, corpus / f"{name}.c", args.timeout)
        results[name] = result
        print(f"{'PASS' if result['accepted'] else 'FAIL'} {name}")
    accepted = sum(bool(r["accepted"]) for r in results.values())
    report = {
        "corpus": str(corpus),
        "checker": str(checker),
        "total": len(NAMES),
        "accepted": accepted,
        "results": results,
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"S9 acceptance: {accepted}/{len(NAMES)}; details: {args.report}")
    if not results["isblank"]["accepted"]:
        print("REGRESSION: unchanged musl isblank must type-check", file=sys.stderr)
        return 1
    if accepted < args.min_accepted:
        print(f"REGRESSION: acceptance below {args.min_accepted}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
