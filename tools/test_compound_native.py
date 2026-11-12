#!/usr/bin/env python3
"""Run original defined-behavior C99 automatic compound-literal programs.

Each case is compiled, linked and executed independently with GCC and Mini-C.
Require zero exit and clean stdout/stderr on both paths.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

CASES = {
    "scalar_lvalue_address": r"""
        int main(void) {
            int *p = &(int){7};
            if (*p != 7) return 1;
            *p += 5;
            return *p != 12;
        }
    """,
    "array_decay_and_index": r"""
        int main(void) {
            int *p = (int[]){2, 5, 8};
            return p[0] != 2 || p[1] != 5 || p[2] != 8
                || (int[]){3, 9, 1}[1] != 9;
        }
    """,
    "struct_postfix_member": r"""
        struct Pair { int left; int right; };
        int main(void) {
            return (struct Pair){.right=7,.left=4}.left != 4
                || (struct Pair){.right=7,.left=4}.right != 7;
        }
    """,
    "struct_copy_and_assignment": r"""
        struct Pair {int left; int right;};
        int main(void) {
            struct Pair p = (struct Pair){.right=6,.left=3};
            struct Pair q = {0, 0};
            q = (struct Pair){.right=11,.left=9};
            return p.left!=3 || p.right!=6 || q.left!=9 || q.right!=11;
        }
    """,
    "typedef_array_compound": r"""
        typedef int Triple[3];
        int main(void) {
            int *p=(Triple){4, 5, 6};
            return p[0]!=4 || p[1]!=5 || p[2]!=6;
        }
    """,
    "scalar_braces": r"""
        int main(void) {
            return (int){9} != 9 || (long){12} != 12;
        }
    """,
    "sizeof_unevaluated": r"""
        int calls;
        int bump(void) { calls++; return 5; }
        int main(void) {
            unsigned long a=sizeof (int[]){bump(), bump(), bump()};
            unsigned long b=sizeof((int[]){bump(),bump()});
            return a!=3*sizeof(int) || b!=2*sizeof(int) || calls!=0;
        }
    """,
    "short_circuit_skips_initializer": r"""
        int calls;
        int bump(void) { calls++; return 6; }
        int main(void) {
            int result=0 && (int){bump()};
            return result!=0 || calls!=0;
        }
    """,
    "mixed_nested_designators": r"""
        struct Packet {int array[3]; int count;};
        int main(void) {
            struct Packet *p = &(struct Packet){.array[2]=8,.array[0]=4,.count=2};
            return p->array[0]!=4 || p->array[1]!=0
                || p->array[2]!=8 || p->count!=2;
        }
    """,
    "repeated_loop_evaluation": r"""
        int calls;
        int bump(void) {calls++; return calls;}
        int main(void) {
            int total=0;
            for (int i=0;i<3;i++) {
                int *p=(int[]){bump(), 9};
                total += p[0];
            }
            return calls!=3 || total!=6;
        }
    """,
    "pointer_call_argument": r"""
        int read(int *p) {return p[0]+p[2];}
        int main(void) {return read((int[]){3,5,7}) != 10;}
    """,
    "compound_ternary_branch": r"""
        int choose(int yes) {
            int *p = yes ? (int[]){4, 7} : (int[]){2, 3};
            return p[0] + p[1];
        }
        int main(void) {return choose(1)!=11 || choose(0)!=5;}
    """,
    "inline_anonymous_struct": r"""
        int main(void) {
            return ((struct {int a;long b;}){.a=3,.b=7}).a!=3
                || ((struct {int a;long b;}){.a=3,.b=7}).b!=7;
        }
    """,
    "inline_anonymous_union": r"""
        int main(void) {
            return ((union {int a;long b;}){.b=17}).b!=17;
        }
    """,
    "inline_typename_with_existing_tag_member": r"""
        struct Field {int value;};
        int main(void) {
            return ((struct {struct Field field;int tag;})
                {.field={.value=9},.tag=5}).field.value!=9;
        }
    """,
    "inline_named_tag_reused_after_compound": r"""
        int main(void) {
            int first=((struct Introduced {int a;long b;})
                {.a=3,.b=5}).a;
            struct Introduced second={7,11};
            return first!=3 || second.a!=7 || second.b!=11;
        }
    """,
    "inline_named_union_reused_after_compound": r"""
        int main(void) {
            long first=((union Introduced {int a;long b;}){.b=13}).b;
            union Introduced second={.b=17};
            return first!=13 || second.b!=17;
        }
    """,
    "inline_tag_shadows_enclosing_scope": r"""
        struct Outer {long a;};
        int main(void) {
            struct Outer old={3};
            {
                int inner=((struct Outer {int b;}){.b=5}).b;
                struct Outer other={7};
                if (inner!=5 || other.b!=7 || sizeof(other)!=4) return 1;
            }
            return old.a!=3 || sizeof(old)!=8;
        }
    """,
    "sizeof_inline_anonymous_type": r"""
        int main(void) {
            return sizeof(struct {char c;long n;})!=16
                || sizeof(union {char c;long n;})!=8;
        }
    """,
    "sizeof_named_tag_defines_type": r"""
        int main(void) {
            int count=sizeof(struct Foo {int x;long y;});
            struct Foo value={3,5};
            return count!=16 || value.x!=3 || value.y!=5;
        }
    """,
    "inline_scalar_field_address": r"""
        int main(void) {
            int *p=&((struct {int a;int b;}){.a=3,.b=8}).b;
            long *q=&((union {int a;long b;}){.b=17}).b;
            return *p!=8 || *q!=17;
        }
    """,
    "inline_dynamic_initializer_order": r"""
        int calls;
        int bump(void) {calls++;return calls;}
        int main(void) {
            int first=((struct {int x;int y;}){.y=bump(),.x=bump()}).x;
            return calls!=2 || first!=2;
        }
    """,
    "sizeof_inline_compound_skips_calls": r"""
        int calls;
        int bump(void) {calls++;return 4;}
        int main(void) {
            unsigned long size=sizeof (struct {int x;int y;})
                {.x=bump(),.y=bump()};
            return size!=8 || calls!=0;
        }
    """,
    "nested_inline_aggregate_types": r"""
        int main(void) {
            int result=((struct {
                struct {int left;int right;} nested;
                int extra;
            }){.nested={.left=3,.right=7},.extra=9}).nested.right;
            return result!=7;
        }
    """,
}


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, capture_output=True,
                          check=False, timeout=45)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path,
                        default=ROOT / "target/debug/mini-c-cranelift")
    args = parser.parse_args()
    backend = args.backend.resolve()
    with tempfile.TemporaryDirectory(prefix="mini-c-compound-") as dirname:
        tmp = Path(dirname)
        for name, source in CASES.items():
            src = tmp / f"{name}.c"
            obj = tmp / f"{name}.o"
            gcc = tmp / f"{name}.gcc"
            mini = tmp / f"{name}.mini"
            src.write_text(source, encoding="utf-8")
            commands = [
                ["cc", "-std=c99", "-pedantic-errors", "-O0", str(src), "-o", str(gcc)],
                [str(backend), "--emit-obj", str(obj), str(src)],
                ["cc", str(obj), "-o", str(mini)],
                [str(gcc)],
                [str(mini)],
            ]
            for command in commands:
                result = run(command)
                if result.returncode != 0 or result.stdout or result.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} code={result.returncode}; "
                        f"stdout={result.stdout!r} stderr={result.stderr!r}"
                    )
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: expected native ELF object")
            print(f"{name}: GCC/Mini-C compound literal execution PASS")
    print(f"C99 compound literal native: {len(CASES)} cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
