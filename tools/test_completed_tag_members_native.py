#!/usr/bin/env python3
"""GCC-versus-Mini-C native regression tests for completed tagged members (C99)."""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "named_struct_by_value_auto": r"""
        struct Inner { int a; long b; };
        struct Outer { char flag; struct Inner payload; int marker; };
        int main(void) {
            struct Outer o={.flag=3,.payload={.b=9,.a=5},.marker=7};
            return o.flag!=3 || o.payload.a!=5 || o.payload.b!=9
                || o.marker!=7 || sizeof(o)!=32;
        }
    """,
    "named_struct_by_value_global": r"""
        struct Inner { int a; long b; };
        struct Outer { char flag; struct Inner payload; };
        struct Outer global={.payload={.b=13,.a=5},.flag=2};
        int main(void) {
            return global.flag!=2 || global.payload.a!=5
                || global.payload.b!=13 || sizeof(global)!=24;
        }
    """,
    "completed_nested_designation": r"""
        struct Inner {int a;long b;};
        struct Holder {struct Inner first; struct Inner second;};
        struct Holder h={.second.b=15,.first.a=3,.first.b=5,.second.a=11};
        int main(void) {
            return h.first.a!=3 || h.first.b!=5
                || h.second.a!=11 || h.second.b!=15;
        }
    """,
    "array_of_tagged_struct_members": r"""
        struct Element {int x;long y;};
        struct Group {struct Element items[3];int count;};
        struct Group g={.items[2].y=8,.items[0].x=3,.count=3};
        int main(void) {
            return g.items[0].x!=3 || g.items[0].y!=0
                || g.items[1].x!=0 || g.items[2].y!=8
                || g.count!=3 || sizeof(g)!=56;
        }
    """,
    "named_union_inside_struct": r"""
        union Data {int i;long l;};
        struct Record {char code;union Data data;};
        struct Record record={.code=3,.data={.l=0x123456L}};
        int main(void) {
            return record.code!=3 || record.data.l!=0x123456L
                || sizeof(record)!=16;
        }
    """,
    "struct_inside_union": r"""
        struct Inner {int a;long b;};
        union Carrier {struct Inner record;long raw[2];};
        union Carrier u={.record={.a=4,.b=9}};
        int main(void) {
            return u.record.a!=4 || u.record.b!=9 || sizeof(u)!=16;
        }
    """,
    "typedef_and_const_qualified_tag": r"""
        typedef struct Pair {int left;long right;} Pair;
        struct Holder {Pair mutable;const Pair fixed;};
        int main(void) {
            struct Holder h={.mutable={3,5},.fixed={7,11}};
            return h.mutable.left!=3 || h.mutable.right!=5
                || h.fixed.left!=7 || h.fixed.right!=11
                || sizeof(h)!=32;
        }
    """,
    "multiple_comma_members": r"""
        struct Vec {int x,y,z;};
        struct Holder {struct Vec first,second;int tag;};
        int main(void) {
            struct Holder h={.first={1,2,3},.second={4,5,6},.tag=7};
            return h.first.y!=2 || h.second.z!=6
                || h.tag!=7 || sizeof(h)!=28;
        }
    """,
    "nested_copy_assignment": r"""
        struct Pair {int a;long b;};
        struct Wrapper {struct Pair inner;int tag;};
        int main(void) {
            struct Wrapper left={.inner={2,4},.tag=6};
            struct Wrapper right={.inner={0,0},.tag=0};
            right=left;
            return right.inner.a!=2 || right.inner.b!=4 || right.tag!=6;
        }
    """,
    "nested_three_named_types": r"""
        struct A {int value;};
        struct B {struct A a;int mark;};
        struct C {struct B b;};
        int main(void) {
            struct C c={{{17},5}};
            return c.b.a.value!=17 || c.b.mark!=5 || sizeof(c)!=8;
        }
    """,
    "self_referential_pointer": r"""
        struct Node;
        struct Node {int value;struct Node *next;};
        struct Node first={3,0};
        struct Node second={8,&first};
        int main(void) {
            return second.next->value!=3 || first.value!=3;
        }
    """,
    "block_scoped_tag_type": r"""
        struct OuterType {int value;};
        int main(void) {
            struct OuterType before={3};
            {
                struct OuterType;
                struct OuterType {long value;};
                struct Local {struct OuterType nested; int extra;};
                struct Local local={.nested={11},.extra=5};
                if (local.nested.value!=11 || local.extra!=5) return 1;
            }
            return before.value!=3;
        }
    """,
}


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, capture_output=True, check=False, timeout=60)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path, default=ROOT / "target/debug/mini-c-cranelift")
    args = parser.parse_args()
    backend = args.backend.resolve()
    with tempfile.TemporaryDirectory(prefix="mini-c-completed-tag-") as dirname:
        base = Path(dirname)
        for name, source in CASES.items():
            src = base / f"{name}.c"
            obj = base / f"{name}.o"
            gcc = base / f"{name}.gcc"
            mini = base / f"{name}.mini"
            src.write_text(source, encoding="utf-8")
            for command in (
                ["cc", "-std=c99", "-pedantic-errors", "-O0", str(src), "-o", str(gcc)],
                [str(backend), "--emit-obj", str(obj), str(src)],
                ["cc", str(obj), "-o", str(mini)],
                [str(gcc)],
                [str(mini)],
            ):
                result = run(command)
                if result.returncode != 0 or result.stdout or result.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} code={result.returncode}; "
                        f"stdout={result.stdout!r} stderr={result.stderr!r}"
                    )
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: missing ELF object")
            print(f"{name}: GCC/Mini-C completed tagged member execution PASS")
        print(f"completed tagged aggregate members: {len(CASES)} cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
