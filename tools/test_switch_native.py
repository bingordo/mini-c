#!/usr/bin/env python3
"""Execute source-pinned C99 switch programs with GCC and Mini-C native ELF.

These tests verify *behavior*, not merely object emission or semantic
acceptance. All cases return zero on success and run with the internal
preprocessor's default driver behavior.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "fallthrough_and_default": r"""
        int choose(int x) {
            int total=0;
            switch(x) {
                case 1: total=3; break;
                case 2: total=4;
                case 3: total+=5; break;
                default: total=6;
            }
            return total;
        }
        int main(void) {
            return choose(1)!=3 || choose(2)!=9
                || choose(3)!=5 || choose(99)!=6;
        }
    """,
    "loop_continue_and_nearest_break": r"""
        int main(void) {
            int result=2;
            for (int i=0; i<4; i++) {
                switch(i) {
                    case 1: continue;
                    default: result+=i;
                }
                result+=3;
            }
            return result != 16;
        }
    """,
    "nested_switch_and_shared_labels": r"""
        int main(void) {
            int value=0;
            switch (1) {
                case 0: value=100; break;
                case 1:
                    switch(2) {
                        case 1: value=8; break;
                        case 2: value=7; break;
                        default: value=9;
                    }
                    value+=3;
                    break;
                case 3: case 4: value=11; break;
                default: value=12;
            }
            return value!=10;
        }
    """,
    "unsigned_long_maximum_case": r"""
        int check(unsigned long x) {
            switch(x) {
                case 18446744073709551615UL: return 1;
                case 0UL: return 2;
                default: return 3;
            }
        }
        int main(void) {
            return check(~0UL)!=1 || check(0UL)!=2 || check(8UL)!=3;
        }
    """,
    "unlabelled_empty_and_default_only": r"""
        int check(int x) {
            int result=2;
            switch(x) { result=99; }
            switch(x) { default: result+=5; }
            return result;
        }
        int main(void) { return check(2)!=7; }
    """,
    "branch_in_arm": r"""
        int test(int x) {
            int v=0;
            switch(x) {
                case 0: if (x) v=7; else v=2; break;
                case 1: v=3; break;
                default: v=4;
            }
            return v;
        }
        int main(void) { return test(0)!=2 || test(1)!=3 || test(2)!=4; }
    """,
}
UNSUPPORTED = {
    "nested_case": (
        "int test(int x){ switch(x){ if(x){case 1:return 1;} "
        "default:return 0;} }",
        "native switch labels under nested",
    ),
    "jump_past_local_definition": (
        "int test(int x){switch(x){int y=3;case 1:return y;"
        "default:return 0;}}",
        "cannot jump over a local declaration across case labels",
    ),
}


def run(argv: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(argv, text=True, capture_output=True,
                          timeout=45, check=False)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path,
                        default=ROOT / "target/debug/mini-c-cranelift")
    args = parser.parse_args()
    backend = args.backend.resolve()
    with tempfile.TemporaryDirectory(prefix="mini-c-switch-native-") as tmp:
        base = Path(tmp)
        for name, source in CASES.items():
            src = base / f"{name}.c"
            mini_obj = base / f"{name}.o"
            mini_exe = base / f"{name}.mini"
            gcc_exe = base / f"{name}.gcc"
            src.write_text(source, encoding="utf-8")
            steps = [
                ["cc", "-std=c99", "-pedantic-errors", "-Wno-switch-unreachable", "-O0",
                 str(src), "-o", str(gcc_exe)],
                [str(backend), "--emit-obj", str(mini_obj), str(src)],
                ["cc", str(mini_obj), "-o", str(mini_exe)],
                [str(gcc_exe)],
                [str(mini_exe)],
            ]
            for command in steps:
                proc = run(command)
                if proc.returncode != 0 or proc.stdout or proc.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} returned "
                        f"{proc.returncode}; stdout={proc.stdout!r}; "
                        f"stderr={proc.stderr!r}"
                    )
            if not mini_obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: missing native ELF header")
            print(f"{name}: GCC/Mini-C native execution PASS")
        for name, (source, expected) in UNSUPPORTED.items():
            src = base / f"{name}.c"
            obj = base / f"{name}.o"
            src.write_text(source, encoding="utf-8")
            result = run([str(backend), "--emit-obj", str(obj), str(src)])
            if result.returncode == 0 or expected not in result.stderr:
                raise AssertionError(
                    f"{name}: expected explicit rejection '{expected}', "
                    f"got exit={result.returncode}, stderr={result.stderr!r}"
                )
            print(f"{name}: explicit unsupported form rejected")
    print(f"Switch native differential: {len(CASES)} executable cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
