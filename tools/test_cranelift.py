#!/usr/bin/env python3
"""Compare Mini-C Cranelift-generated ELF objects with GCC reference executions.

Requires previously built binaries: target/debug/mini-c-cranelift and
target/debug/mini-c-fuzz. Build them once via Cargo; this script never reruns
Cargo or downloads a compiler. Failures preserve the C and command outputs.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

PROGRAMS = {
    "arithmetic": "int main(void) { int x=7; x=x*3-2; return x/2+x%2; }",
    "stack_array": """int main(void) {
        int a[4]; a[0]=3; a[1]=7; a[2]=11; a[3]=19;
        return a[1] + a[3];
    }""",
    "alias_scalar": """int main(void) {
        int x=5; int *p=&x; *p=18; return x;
    }""",
    "indexed_pointer": """int update(int *p, int idx) {
        p[idx] = 9; return *(idx+p);
    }
    int main(void) { int a[3]; a[0]=1; a[1]=2; a[2]=3;
        return update(a, 1) + a[1];
    }""",
    "multi_array": """int main(void) {
        int a[2][3]; a[0][2]=4; a[1][0]=6; a[1][2]=9;
        int *row=a[1]; return row[0]+row[2]+a[0][2];
    }""",
    "nested": """int count(int limit) {
        int i=0; int total=0;
        while (i<limit) {
            if (i==3) { i=i+1; continue; }
            if (i==7) break;
            { int total=1; if (total != 1) return 99; }
            total=total+i; i=i+1;
        }
        return total;
    } int main(void) { return count(12); }""",
    "recursive": """int factorial(int n) {
        if(n<=1) return 1;
        return n*factorial(n-1);
    } int main(void) { return factorial(5); }""",
    "mutual": """int odd(int); int even(int);
    int odd(int x) { if (x==0) return 0; return even(x-1); }
    int even(int x) { if (x==0) return 1; return odd(x-1); }
    int main(void) { return odd(9); }""",
    "long": """long wide(long n, int m) { return n / m + n % m; }
    int main(void) { return wide(100, 6); }""",
    "unsigned": """unsigned int add(unsigned int a, unsigned int b) {
        return a + b;
    } int main(void) { return add(7,11); }""",
    "short_circuit": """int main(void) {
        int n=0;
        if (n && (9/n)) return 31;
        if (1 || (9/n)) return 12;
        return 19;
    }""",
    "void_call": """void tick(void) { return; }
    int main(void) { tick(); return 17; }""",
    "prototypes": """int f(int, int);
    int main(void) { return f(5, 6); }
    int f(int x, int y) { return x + y; }""",
    "main_implicit_zero": "int main(void) { int x=2; x=x+1; }",
    "multiple_return": """int absval(int x) {
        if (x < 0) return -x;
        return x;
    } int main(void) { return absval(-7); }""",
    "signed_unsigned_comparison": """int gt(unsigned long a, unsigned long b) {
        return a > b;
    } int main(void) { return gt(5,3); }""",
    "loop_inside_branch": """int main(void) {
        int x=0; int total=0;
        if (1) { while (x<5) { total=total+x; x=x+1; } }
        else { return 99; }
        return total;
    }""",
    "external_libc": "int abs(int); int main(void) { return abs(-25); }",
    "static_function": "static int next(int x) { return x + 1; } int main(void) { return next(6); }",
    "global_initialized": "int g=7; int main(void) { return g; }",
    "global_mutation": "int g=10; int bump(void) { g=g+1; return g; } int main(void) { return bump(); }",
    "global_tentative": "int g; int main(void) { g=19; return g; }",
    "global_static": "static int g=9; extern int g; int main(void) { return g; }",
    "global_shadowing": "int g=13; int main(void) { int g=7; return g; }",
    "global_redeclaration": "extern int g; int g=42; int main(void) { return g; }",
    "global_array": "int a[4]; int main(void) { a[1]=17; return a[1]+a[0]; }",
    "global_multidim_array": "int a[2][3]; int main(void) { a[1][2]=29; return a[1][2]; }",
    "global_address_taken": "int g=3; int main(void) { int *p=&g; *p=21; return g; }",
    "global_narrow_char": "unsigned char g=255; int main(void) { return g; }",
    "global_null_pointer": "int *p=0; int main(void) { return p==0; }",
    "global_tentative_pointer": "int *p; int main(void) { return p==0; }",


}


def run(command: list[str], timeout: int = 30) -> dict:
    try:
        done = subprocess.run(command, capture_output=True, text=True, timeout=timeout)
        return {
            "argv": command, "returncode": done.returncode,
            "stdout": done.stdout, "stderr": done.stderr,
        }
    except (OSError, subprocess.TimeoutExpired) as exc:
        return {"argv": command, "error": str(exc)}


def test_case(name: str, src: str, args: argparse.Namespace, expected: int | None = None) -> bool:
    case = args.artifacts / name
    case.mkdir(parents=True, exist_ok=True)
    (case / "input.c").write_text(src + "\n")
    exe = case / "mini-c.exe"
    ref = case / "gcc.exe"
    obj = case / "mini-c.o"
    commands = [
        [str(args.backend), "--emit-obj", str(obj), str(case / "input.c")],
        [args.cc, str(obj), "-o", str(exe)],
        [args.cc, "-std=c11", "-O2", str(case / "input.c"), "-o", str(ref)],
        [str(exe)],
        [str(ref)],
    ]
    record = {"name": name, "commands": [], "compiler_version": run([args.cc, "--version"])}
    for index, cmd in enumerate(commands):
        result = run(cmd)
        record["commands"].append(result)
        (case / "results.json").write_text(json.dumps(record, indent=2) + "\n")
        if index < 3 and result.get("returncode") != 0:
            print(f"FAIL {name}: command #{index+1}: {result}", file=sys.stderr)
            return False
        if index >= 3 and "returncode" not in result:
            print(f"FAIL {name}: execution failed: {result}", file=sys.stderr)
            return False
    mini, gcc = record["commands"][3:5]
    # A matching signal crash is not a successful differential comparison.
    if (
        mini["returncode"] < 0 or gcc["returncode"] < 0
        or (expected is not None and gcc["returncode"] != expected)
        or mini["returncode"] != gcc["returncode"]
        or mini["stdout"] != gcc["stdout"]
    ):
        print(f"FAIL {name}: Mini-C {mini}, GCC {gcc}, expected={expected}", file=sys.stderr)
        return False
    shutil.rmtree(case)
    return True



def mixed_objects(name: str, mini_source: str, gcc_source: str, args: argparse.Namespace) -> bool:
    """Validate SysV function ABI in both linking directions."""
    directory = args.artifacts / name
    directory.mkdir(parents=True, exist_ok=True)
    mini_input = directory / "mini.c"
    gcc_input = directory / "gcc.c"
    mini_input.write_text(mini_source + "\n")
    gcc_input.write_text(gcc_source + "\n")
    obj = directory / "mini.o"
    executable = directory / "combined.exe"
    reference = directory / "reference.exe"
    commands = [
        [str(args.backend), "--emit-obj", str(obj), str(mini_input)],
        [args.cc, str(obj), str(gcc_input), "-o", str(executable)],
        [args.cc, "-std=c11", "-O2", str(mini_input), str(gcc_input), "-o", str(reference)],
        [str(executable)],
        [str(reference)],
    ]
    records = []
    for idx, command in enumerate(commands):
        result = run(command)
        records.append(result)
        (directory / "results.json").write_text(
            json.dumps({"case": name, "commands": records}, indent=2) + "\n"
        )
        if idx < 3 and result.get("returncode") != 0:
            print(f"FAIL {name}: link/compile: {result}", file=sys.stderr)
            return False
        if idx >= 3 and "returncode" not in result:
            print(f"FAIL {name}: execution: {result}", file=sys.stderr)
            return False
    if (
        records[-2]["returncode"] < 0 or records[-1]["returncode"] < 0
        or records[-2]["returncode"] != records[-1]["returncode"]
        or records[-2]["stdout"] != records[-1]["stdout"]
    ):
        print(f"FAIL {name}: ABI mismatch: {records[-2]} vs {records[-1]}", file=sys.stderr)
        return False
    shutil.rmtree(directory)
    return True

def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--backend", type=Path, default=Path("target/debug/mini-c-cranelift"))
    p.add_argument("--fuzzer", type=Path, default=Path("target/debug/mini-c-fuzz"))
    p.add_argument("--seed-count", type=int, default=4)
    p.add_argument("--steps", type=int, default=100)
    p.add_argument("--cc", default="cc")
    p.add_argument("--artifacts", type=Path, default=Path("target/cranelift-failures"))
    args = p.parse_args()
    if args.seed_count < 0 or args.steps < 0:
        p.error("seed-count and steps must be nonnegative")
    args.backend = args.backend.resolve()
    args.fuzzer = args.fuzzer.resolve()
    args.artifacts = args.artifacts.resolve()
    if shutil.which(args.cc) is None or not args.backend.is_file():
        print("build the backend and ensure cc is installed", file=sys.stderr)
        return 2
    if args.seed_count and not args.fuzzer.is_file():
        print("build mini-c-fuzz before running seeded differential tests", file=sys.stderr)
        return 2
    for name, program in PROGRAMS.items():
        if not test_case(name, program, args):
            print(f"Failure reproducer retained: {args.artifacts / name}", file=sys.stderr)
            return 1
    mixed = [
        ("gcc_calls_mini", "int kernel(int x, int y) { return x * y + y; }",
         "int kernel(int,int); int main(void) { return kernel(6,7); }"),
        ("mini_calls_gcc", "int supply(int); int main(void) { return supply(20); }",
         "int supply(int x) { return x + 3; }"),
        ("mini_reads_gcc_global",
         "extern int shared; int main(void) { return shared; }",
         "int shared=27;"),
        ("mini_writes_gcc_global",
         "extern int shared; int main(void) { shared=31; return shared; }",
         "int shared=5;"),
        ("gcc_reads_mini_global",
         "int shared=23;",
         "extern int shared; int main(void) { return shared; }"),
        ("gcc_writes_mini_global",
         "int shared;",
         "extern int shared; int main(void) { shared=37; return shared; }"),
        ("mini_static_global_not_exported",
         "static int hidden=11; int get_hidden(void) { return hidden; }",
         "int get_hidden(void); int main(void) { return get_hidden(); }"),
    ]
    for name, mini_source, gcc_source in mixed:
        if not mixed_objects(name, mini_source, gcc_source, args):
            print(f"Failure reproducer retained: {args.artifacts / name}", file=sys.stderr)
            return 1
    for seed in range(args.seed_count):
        generated = run([str(args.fuzzer), str(seed), str(args.steps)])
        if generated.get("returncode") != 0:
            print(f"FAIL generating seed={seed}: {generated}", file=sys.stderr)
            return 1
        if not test_case(f"seed-{seed}", generated["stdout"], args, expected=1):
            print(f"Failure reproducer retained: {args.artifacts / f'seed-{seed}'}", file=sys.stderr)
            return 1
    print(f"PASS {len(PROGRAMS)} standalone fixtures, {len(mixed)} mixed-object ABI cases, "
          f"and {args.seed_count} generated programs")
    return 0


if __name__ == "__main__":
    sys.exit(main())
