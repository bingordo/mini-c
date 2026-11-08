#!/usr/bin/env python3
"""Source-only ISO C99 acceptance/constraint differential against GCC and Clang.

The feature manifest is intentionally independent of Mini-C's AST, IR, and
backends. A 'gap' is accounted as coverage debt, *never* as a C99 pass.
Everything uses real source files and -std=c99 -pedantic-errors; success alone
cannot establish complete semantic equivalence, so checked-type witnesses
are encoded as C99 constant-expression/array-bound constraints where possible.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


def generate(source: str, seed: int, variant: int, case_id: str) -> str:
    """Deterministic independently seeded bounded grammar-value variation."""
    tag = int.from_bytes(hashlib.sha256(
        f"{seed}:{variant}:{case_id}".encode()).digest()[:8], "little")
    number = 2 + tag % 5
    return source.replace("@N@", str(number)) + "\n"


def validate_manifest(manifest: dict) -> list[dict]:
    if manifest.get("schema_version") != 1:
        raise ValueError("unsupported C99 manifest schema")
    cases = manifest.get("cases")
    if not isinstance(cases, list) or not cases:
        raise ValueError("empty C99 feature matrix")
    seen: set[str] = set()
    for case in cases:
        if not isinstance(case, dict):
            raise ValueError("invalid feature row")
        cid = case.get("id")
        if not isinstance(cid, str) or not re.fullmatch(r"[a-z][a-z0-9-]*", cid):
            raise ValueError(f"invalid case id: {cid!r}")
        if cid in seen:
            raise ValueError(f"duplicate C99 feature case id {cid}")
        seen.add(cid)
        if case.get("expect") not in ("accept", "reject"):
            raise ValueError(f"{cid}: expected normative acceptance missing")
        if case.get("mini") not in ("supported", "gap"):
            raise ValueError(f"{cid}: unsupported Mini-C status")
        if case["expect"] == "reject" and case["mini"] == "gap":
            raise ValueError(f"{cid}: gap classification is only for valid C99")
        if case.get("kind") not in ("valid", "constraint"):
            raise ValueError(f"{cid}: explicit standards category required")
        if (case["kind"] == "valid") != (case["expect"] == "accept"):
            raise ValueError(f"{cid}: expected/category mismatch")
        if not isinstance(case.get("test_ref"), str) or not case["test_ref"]:
            raise ValueError(f"{cid}: missing stable test or plan reference")
        if not isinstance(case.get("feature"), str) or not case["feature"].startswith("F"):
            raise ValueError(f"{cid}: missing milestone")
        if not isinstance(case.get("source"), str) or not case["source"].strip():
            raise ValueError(f"{cid}: empty source")
        # Fixed normative constraint fixtures deliberately need no placeholder.
        # Generated valid cases may contain @N@ for bounded variations.
    return cases


def invoke(argv: list[str], timeout: float) -> dict:
    try:
        cp = subprocess.run(argv, text=True, capture_output=True,
                            timeout=timeout, check=False)
        return {"command": argv, "exit_code": cp.returncode,
                "stdout": cp.stdout[-4000:], "stderr": cp.stderr[-4000:],
                "accepted": cp.returncode == 0}
    except subprocess.TimeoutExpired:
        return {"command": argv, "timeout_seconds": timeout,
                "accepted": False, "timed_out": True}


def version(executable: str) -> str:
    result = invoke([executable, "--version"], 8)
    return (result.get("stdout") or result.get("stderr") or "").splitlines()[0:1][0] if (
        result.get("stdout") or result.get("stderr")) else "unavailable"


def check_one(path: Path, gcc: str, clang: str, mini: str, timeout: float) -> dict:
    return {
        "gcc": invoke([gcc, "-std=c99", "-pedantic-errors", "-fsyntax-only",
                       "-x", "c", str(path)], timeout),
        "clang": invoke([clang, "-std=c99", "-pedantic-errors", "-fsyntax-only",
                         "-x", "c", str(path)], timeout),
        "mini_c": invoke([mini, "check", "--cpp=none", str(path)], timeout),
    }


def classify(case: dict, results: dict) -> tuple[bool, str]:
    if any(result.get("timed_out") for result in results.values()):
        return False, "timeout"
    expected = case["expect"] == "accept"
    if any(results[name]["accepted"] != expected for name in ("gcc", "clang")):
        return False, "reference-disagreement"
    if case["mini"] == "gap":
        return True, ("gap-resolved" if results["mini_c"]["accepted"] else "known-gap")
    if results["mini_c"]["accepted"] != expected:
        return False, "mini-c-differential"
    return True, "matched"


def reproduces(case: dict, path: Path, gcc: str, clang: str, mini: str,
               timeout: float, original_reason: str) -> bool:
    outcomes = check_one(path, gcc, clang, mini, timeout)
    _, reason = classify(case, outcomes)
    return reason == original_reason


def minimize_lines(case: dict, source: str, path: Path, gcc: str, clang: str,
                   mini: str, timeout: float, reason: str, budget: int = 24) -> str:
    """Bounded deletion reducer; only keep a removal reproducing the *same*
    classification with the independent reference verdict intact."""
    lines = source.splitlines(keepends=True)
    if len(lines) <= 1:
        return source
    stride = max(1, len(lines) // 2)
    while stride and budget > 0:
        made_progress = False
        for start in range(0, len(lines), stride):
            if budget <= 0:
                break
            candidate = lines[:start] + lines[start + stride:]
            if not candidate or candidate == lines:
                continue
            path.write_text("".join(candidate))
            budget -= 1
            if reproduces(case, path, gcc, clang, mini, timeout, reason):
                lines = candidate
                made_progress = True
                break
        if not made_progress:
            stride //= 2
    return "".join(lines)


def run(args: argparse.Namespace) -> int:
    cases = validate_manifest(json.loads(args.manifest.read_text()))
    if args.variants < 1 or args.variants > 32:
        raise ValueError("--variants must be in 1..32")
    args.report.parent.mkdir(parents=True, exist_ok=True)
    versions = {k: version(v) for k, v in (
        ("gcc", args.gcc), ("clang", args.clang), ("mini_c", args.mini))}
    records: list[dict] = []
    failures = 0
    with tempfile.TemporaryDirectory(prefix="mini-c-c99-f0-") as td:
        path = Path(td) / "probe.c"
        for case in cases:
            for variant in range(args.variants):
                source = generate(case["source"], args.seed, variant, case["id"])
                path.write_text(source)
                outcomes = check_one(path, args.gcc, args.clang, args.mini, args.timeout)
                passed, reason = classify(case, outcomes)
                record = {
                    "id": case["id"], "feature": case["feature"],
                    "variant": variant, "seed": args.seed,
                    "category": case["kind"], "expected": case["expect"],
                    "mini_baseline": case["mini"], "source_sha256": hashlib.sha256(
                        source.encode()).hexdigest(), "passed": passed,
                    "classification": reason,
                    "observations": outcomes,
                }
                records.append(record)
                if not passed:
                    failures += 1
                    out = args.artifacts / f"{case['id']}-{variant:02d}"
                    out.mkdir(parents=True, exist_ok=True)
                    (out / "source.c").write_text(source)
                    (out / "case.json").write_text(json.dumps(record, indent=2) + "\n")
                    if args.minimize:
                        minimal = minimize_lines(
                            case, source, path, args.gcc, args.clang,
                            args.mini, args.timeout, reason)
                        (out / "minimized.c").write_text(minimal)
                print(f"C99_F0 {case['id']} variant={variant} "
                      f"classification={reason}", flush=True)
    counts = {reason: sum(r["classification"] == reason for r in records)
              for reason in sorted({r["classification"] for r in records})}
    report = {
        "schema_version": 1, "seed": args.seed, "variants": args.variants,
        "tools": versions, "total": len(records), "unexpected": failures,
        "counts": counts, "cases": records,
    }
    args.report.write_text(json.dumps(report, indent=2) + "\n")
    print("C99_F0_SUMMARY " + json.dumps({
        "total": len(records), "unexpected": failures, "counts": counts,
        "report": str(args.report)}, sort_keys=True), flush=True)
    return 1 if failures else 0


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--manifest", type=Path, default=root / "tools/c99_features.json")
    p.add_argument("--mini", default=str(root / "target/debug/mini-c"))
    p.add_argument("--gcc", default="gcc")
    p.add_argument("--clang", default="clang")
    p.add_argument("--seed", type=int, default=99)
    p.add_argument("--variants", type=int, default=2)
    p.add_argument("--timeout", type=float, default=8)
    p.add_argument("--report", type=Path, default=root / "target/c99-f0/report.json")
    p.add_argument("--artifacts", type=Path, default=root / "target/c99-f0/failures")
    p.add_argument("--minimize", action="store_true",
                   help="bounded line reducer for unexpected mismatches")
    args = p.parse_args()
    try:
        return run(args)
    except (ValueError, OSError) as exc:
        print(f"C99 F0 harness error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
