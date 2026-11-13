#!/usr/bin/env python3
"""C99 native goto differential: compile/link/execute unchanged programs with GCC and Mini-C.

Supports forward jumps over locals, plain compound scopes and labels inside
if/while/do-while/for and labels inside switch arms (with or without cases).
"""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

CASES = {
    "forward_branch": r"""
        int f(int n) {
            int value=9;
            if(n) goto finished;
            value=2;
            finished: return value;
        }
        int main(void) { return f(0)!=2 || f(1)!=9; }
    """,
    "backward_loop": r"""
        int sum_to(int n) {
            int sum=0;
            again: if(n==0) return sum;
            sum+=n; n--; goto again;
        }
        int main(void) { return sum_to(0)!=0 || sum_to(5)!=15; }
    """,
    "forward_after_terminator_and_label_chain": r"""
        int f(void) {
            goto second;
            first: return 1;
            second: third: return 2;
        }
        int main(void) { return f()!=2; }
    """,
    "jump_out_of_nested_if": r"""
        int main(void) {
            int v=0;
            { if(1) {v=3; goto done;} v=7; }
            done: return v!=3;
        }
    """,
    "jump_out_of_switch_arm": r"""
        int f(int n) {
            int result=0;
            switch(n) {
                case 1: result=5; goto done;
                default: result=7;
            }
            done: return result;
        }
        int main(void) { return f(1)!=5 || f(2)!=7; }
    """,
    "typedef_name_label": r"""
        typedef int T;
        int main(void) { T value=4; goto T; value=99; T: return value!=4; }
    """,
    "forward_over_scalar_local": r"""
        int test(int jump) {
            int result=10;
            if(jump) goto target;
            int x=4;
            result+=x;
            goto done;
            target: x=7;
            result+=x;
            done: return result;
        }
        int main(void) { return test(0)!=14 || test(1)!=17; }
    """,
    "skipped_initializer_side_effect": r"""
        int calls=0;
        int side(void) { calls++; return 3; }
        int test(void) { goto target; int x=side(); target: x=9; return x; }
        int main(void) { return test()!=9 || calls!=0; }
    """,
    "forward_over_array_storage": r"""
        int test(void) { goto done; int values[3]; done: values[2]=7; return values[2]; }
        int main(void) { return test()!=7; }
    """,
    "nested_compound_entry": r"""
        int test(int jump) {
            int result=0;
            if(jump) goto inside;
            result=2;
            goto done;
            {
                int scratch=99;
                inside: result+=3;
            }
            done: return result;
        }
        int main(void) { return test(0)!=2 || test(1)!=3; }
    """,
    "nested_shadowed_local": r"""
        int test(void) {
            int x=5;
            goto inner;
            {
                int x=20;
                inner: x=9;
            }
            return x;
        }
        int main(void) { return test()!=5; }
    """,
    "nested_typedef_label": r"""
        typedef int Target;
        int test(void) { goto Target; { Target: return 5; } }
        int main(void) { return test()!=5; }
    """,
    "goto_into_if_then_without_condition": r"""
        int calls=0;
        int cond(void) { calls++; return 0; }
        int test(void) { goto inside; if(cond()) { inside: return 7; } return 0; }
        int main(void) { return test()!=7 || calls!=0; }
    """,
    "goto_into_if_else_without_condition": r"""
        int calls=0;
        int cond(void) { calls++; return 1; }
        int test(void) { goto inside; if(cond()) return 1; else { inside: return 8; } }
        int main(void) { return test()!=8 || calls!=0; }
    """,
    "goto_into_if_then_then_fallthrough": r"""
        int test(void) { int x=0; goto inside; if(0) { inside: x=9; } return x; }
        int main(void) { return test()!=9; }
    """,
    "goto_into_while_and_repeat": r"""
        int test(void) { int x=0; goto inside; while(x<3) { inside: x++; } return x; }
        int main(void) { return test()!=3; }
    """,
    "goto_into_do_while_and_repeat": r"""
        int test(void) { int x=0; goto inside; do { inside: x++; } while(x<3); return x; }
        int main(void) { return test()!=3; }
    """,
    "goto_into_for_skips_init": r"""
        int calls=0;
        int init(void) { calls++; return 0; }
        int test(void) {
            int x=0;
            goto inside;
            for(x=init();x<3;x++) { inside: x++; }
            return x;
        }
        int main(void) { return test()!=4 || calls!=0; }
    """,
    "goto_into_for_decl_scope": r"""
        int test(void) {
            int value=0;
            goto inside;
            for(int i=3;i<5;i++) { inside: i=4; value=i; break; }
            return value;
        }
        int main(void) { return test()!=4; }
    """,
    "goto_into_nested_dead_controls": r"""
        int test(void) {
            goto inside;
            if(0) {
                while(0) {
                    inside: return 11;
                }
            }
            return 0;
        }
        int main(void) { return test()!=11; }
    """,
    "goto_into_direct_if_label": r"""
        int test(void) { goto inside; if(0) inside: return 12; return 0; }
        int main(void) { return test()!=12; }
    """,
    "goto_into_switch_arm_skips_control": r"""
        int calls=0;
        int control(void) { calls++; return 1; }
        int test(void) {
            int value=1;
            goto enter;
            switch(control()) {
                case 1: value=100;
                enter: value+=5; break;
                default: value=200;
            }
            return value;
        }
        int main(void) { return test()!=6 || calls!=0; }
    """,
    "goto_into_switch_without_cases": r"""
        int calls=0;
        int control(void) { calls++; return 9; }
        int init(void) { calls++; return 6; }
        int test(void) {
            int result=0;
            goto enter;
            switch(control()) {
                int skipped=init();
                enter: result=7; break;
            }
            return result;
        }
        int main(void) { return test()!=7 || calls!=0; }
    """,
    "goto_into_nested_switch": r"""
        int calls=0;
        int control(void) { calls++; return 1; }
        int test(void) {
            int value=0;
            goto enter;
            switch(control()) {
                case 1:
                    switch(control()) {
                        case 2:
                            enter: value=7; break;
                        default: value=50;
                    }
                    value+=3; break;
                default: value=70;
            }
            return value;
        }
        int main(void) { return test()!=10 || calls!=0; }
    """,
    "goto_between_switch_arms": r"""
        int test(int x) {
            int value=0;
            switch(x) {
                case 1: goto after_second;
                case 2: value=100;
                after_second: value+=4; break;
                default: value=9;
            }
            return value;
        }
        int main(void) { return test(1)!=4 || test(2)!=104 || test(3)!=9; }
    """,
    "goto_into_switch_nested_loop": r"""
        int calls=0;
        int control(void) { calls++; return 1; }
        int test(void) {
            int count=0;
            goto enter;
            switch(control()) {
                case 1:
                    while(count<3) {
                        enter: count++;
                    }
                    break;
                default: count=9;
            }
            return count;
        }
        int main(void) { return test()!=3 || calls!=0; }
    """,
    "dead_if_return_without_fallback": r"""
        int f(void) { goto target; if(0) { target: return 11; } }
        int main(void) { return f()!=11; }
    """,
    "dead_while_return_without_fallback": r"""
        int f(void) { goto target; while(0) { target: return 12; } }
        int main(void) { return f()!=12; }
    """,
    "dead_do_while_return_without_fallback": r"""
        int f(void) { goto target; do { target: return 13; } while(0); }
        int main(void) { return f()!=13; }
    """,
    "dead_for_return_without_fallback": r"""
        int f(void) { goto target; for(int i=0;i<3;i++) { target: return 14; } }
        int main(void) { return f()!=14; }
    """,
    "dead_switch_case_return_without_fallback": r"""
        int calls=0;
        int control(void) { calls++; return 0; }
        int f(void) { goto target; switch(control()) {case 0: target: return 15;} }
        int main(void) { return f()!=15 || calls!=0; }
    """,
    "dead_switch_no_case_return_without_fallback": r"""
        int calls=0;
        int control(void) { calls++; return 0; }
        int f(void) { goto target; switch(control()) {target: return 16;} }
        int main(void) { return f()!=16 || calls!=0; }
    """,
}
UNSUPPORTED = {
    "reachable_missing_return": (
        "int f(int x) { if(x) return 1; }",
        "may reach its end without a return",
    ),
    "undefined_function_label": (
        "int main(void) {goto missing; switch(1) {case 1: return 0;} }",
        "undefined",
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
    with tempfile.TemporaryDirectory(prefix="mini-c-goto-native-") as tmp:
        base = Path(tmp)
        for name, source in CASES.items():
            src = base / f"{name}.c"
            mini_obj = base / f"{name}.o"
            mini_exe = base / f"{name}.mini"
            gcc_exe = base / f"{name}.gcc"
            src.write_text(source, encoding="utf-8")
            commands = [
                ["cc", "-std=c99", "-pedantic-errors", "-Wno-switch-unreachable", "-O0", str(src), "-o", str(gcc_exe)],
                [str(backend), "--emit-obj", str(mini_obj), str(src)],
                ["cc", str(mini_obj), "-o", str(mini_exe)],
                [str(gcc_exe)],
                [str(mini_exe)],
            ]
            for command in commands:
                proc = run(command)
                if proc.returncode != 0 or proc.stdout or proc.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} returned {proc.returncode}; "
                        f"stdout={proc.stdout!r}; stderr={proc.stderr!r}"
                    )
            if not mini_obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: missing native ELF header")
            print(f"{name}: GCC/Mini-C native goto execution PASS")
        for name, (source, expected) in UNSUPPORTED.items():
            src = base / f"{name}.c"
            obj = base / f"{name}.o"
            src.write_text(source, encoding="utf-8")
            proc = run([str(backend), "--emit-obj", str(obj), str(src)])
            if proc.returncode == 0 or expected not in proc.stderr:
                raise AssertionError(
                    f"{name}: expected '{expected}', got code={proc.returncode}, "
                    f"stderr={proc.stderr!r}"
                )
            print(f"{name}: unsupported CFG edge rejected")
    print(f"Goto native differential: {len(CASES)} executable cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
