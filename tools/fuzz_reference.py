#!/usr/bin/env python3
"""Run Mini-C's generated, self-checking C corpus against independent C compilers.

No third-party Python packages required. On failure, preserve the exact source,
compiler invocations, tool versions and outputs under --artifacts.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


EXPECTED_EXIT = 1  # Generated C returns (x == independently calculated value).


def invoke(argv: list[str], timeout: int = 30, env: dict[str, str] | None = None) -> dict:
    try:
        result = subprocess.run(
            argv, capture_output=True, text=True, timeout=timeout,
            check=False, env=env,
        )
        return {
            "command": argv, "returncode": result.returncode,
            "stdout": result.stdout, "stderr": result.stderr,
        }
    except subprocess.TimeoutExpired as exc:
        return {
            "command": argv, "timeout_seconds": timeout,
            "stdout": (exc.stdout or b"").decode(errors="replace")
            if isinstance(exc.stdout, bytes) else (exc.stdout or ""),
            "stderr": (exc.stderr or b"").decode(errors="replace")
            if isinstance(exc.stderr, bytes) else (exc.stderr or ""),
        }
    except OSError as exc:
        return {"command": argv, "error": str(exc)}


def success(result: dict, expected: int = 0) -> bool:
    return result.get("returncode") == expected


def sanitizer_reported(result: dict) -> bool:
    """Detect sanitizer diagnostics even when their exit status equals success (1)."""
    output = result.get("stderr", "") + "\n" + result.get("stdout", "")
    return any(marker in output for marker in (
        "AddressSanitizer", "UndefinedBehaviorSanitizer", "LeakSanitizer",
        "runtime error:", "Sanitizer: ",
    ))


def variants(
    compiler: str, sanitizers: bool, smoke: bool = False
) -> list[tuple[str, list[str]]]:
    # Fast CI deliberately covers a different compiler/optimization pairing
    # with each compiler; the scheduled/manual campaign runs the full matrix.
    if smoke:
        return [("O0", ["-O0"])] if "clang" in Path(compiler).name else [("O2", ["-O2"])]
    outputs = [("O0", ["-O0"]), ("O2", ["-O2"])]
    if sanitizers:
        # GCC's UBSan and Clang's combined ASan/UBSan cover both sanitizers
        # without relying on GCC ASan, which can be unstable on some runners.
        flags = (
            ["-O1", "-fsanitize=address,undefined", "-fno-sanitize-recover=all"]
            if "clang" in Path(compiler).name
            else ["-O1", "-fsanitize=undefined", "-fno-sanitize-recover=all"]
        )
        outputs.append(("sanitized", flags))
    return outputs


def check_seed(seed: int, args: argparse.Namespace, versions: dict) -> bool:
    case_dir = args.artifacts / f"seed-{seed:06d}"
    case_dir.mkdir(parents=True, exist_ok=True)
    source = case_dir / "case.c"
    manifest: dict = {
        "seed": seed, "steps": args.steps, "expected_exit": EXPECTED_EXIT,
        "git_revision": os.environ.get("GITHUB_SHA"),
        "compiler_versions": versions, "runs": [],
        "reproduce": f"cargo run -q -p mini-c-fuzz -- {seed} {args.steps} > case.c",
    }

    def record(result: dict) -> None:
        manifest["runs"].append(result)
        (case_dir / "results.json").write_text(json.dumps(manifest, indent=2) + "\n")

    generation = invoke([str(args.fuzzer), str(seed), str(args.steps)])
    record(generation)
    if not success(generation):
        print(f"FAIL seed={seed}: Mini-C generator/parser: {generation}", file=sys.stderr)
        return False
    source.write_text(generation["stdout"])
    for compiler in args.compilers:
        for variant, flags in variants(compiler, not args.skip_sanitizers, args.smoke):
            executable = case_dir / f"{Path(compiler).name}-{variant}"
            cmd = [
                compiler, "-std=c11", "-pedantic", "-Wall", "-Wextra",
                "-Werror", *flags, str(source), "-o", str(executable),
            ]
            compilation = invoke(cmd, timeout=args.timeout)
            record(compilation)
            if not success(compilation):
                print(f"FAIL seed={seed} {compiler} {variant}: compile", file=sys.stderr)
                return False
            environment = os.environ.copy()
            # A sanitizer crash may otherwise return 1, which is also the
            # generated program's expected success exit code.
            environment["ASAN_OPTIONS"] = "detect_leaks=1:halt_on_error=1:exitcode=86"
            environment["UBSAN_OPTIONS"] = "halt_on_error=1:print_stacktrace=1:exitcode=87"
            execution = invoke([str(executable)], timeout=args.timeout, env=environment)
            record(execution)
            has_sanitizer_error = variant == "sanitized" and sanitizer_reported(execution)
            if not success(execution, EXPECTED_EXIT) or has_sanitizer_error:
                print(
                    f"FAIL seed={seed} {compiler} {variant}: expected exit "
                    f"{EXPECTED_EXIT}, got {execution.get('returncode', 'timeout/error')}; "
                    f"sanitizer_error={has_sanitizer_error}",
                    file=sys.stderr,
                )
                return False
            executable.unlink()
    shutil.rmtree(case_dir)
    return True


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--fuzzer", type=Path, default=Path("target/debug/mini-c-fuzz"))
    p.add_argument("--seed-start", type=int, default=0)
    p.add_argument("--seed-count", type=int, default=12)
    p.add_argument("--steps", type=int, default=120)
    p.add_argument("--compiler", dest="compilers", action="append")
    p.add_argument("--skip-sanitizers", action="store_true")
    p.add_argument("--smoke", action="store_true",
                   help="one compiler/optimization combination per compiler; no sanitizers")
    p.add_argument("--timeout", type=int, default=30)
    p.add_argument("--artifacts", type=Path, default=Path("target/fuzz-artifacts"))
    args = p.parse_args()
    if args.seed_start < 0 or args.seed_count < 1 or not 0 <= args.steps <= 10000:
        p.error("seed-start must be nonnegative, seed-count positive, and steps 0..10000")
    if args.timeout < 1:
        p.error("timeout must be positive")
    args.compilers = args.compilers or ["gcc", "clang"]
    args.fuzzer = args.fuzzer.resolve()
    for compiler in args.compilers:
        if shutil.which(compiler) is None:
            print(f"required reference compiler not found: {compiler}", file=sys.stderr)
            return 2
    if not args.fuzzer.is_file():
        print("build first with: cargo build -p mini-c-fuzz", file=sys.stderr)
        return 2
    versions = {compiler: invoke([compiler, "--version"])["stdout"].splitlines()[:1]
                for compiler in args.compilers}
    args.artifacts.mkdir(parents=True, exist_ok=True)
    total = 0
    for seed in range(args.seed_start, args.seed_start + args.seed_count):
        if not check_seed(seed, args, versions):
            print(f"Reproducer and command logs retained in: {args.artifacts}", file=sys.stderr)
            return 1
        total += 1
        print(f"PASS seed={seed}", flush=True)
    matrix = "one optimization per compiler" if args.smoke else (
        "two optimizations" if args.skip_sanitizers else "two optimizations + sanitizer"
    )
    print(f"PASS {total} seeds, {len(args.compilers)} compilers, {matrix}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
