#!/usr/bin/env python3
"""GCC/Mini-C C99 bit-field aggregate initializer native execution differentials."""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "global_positional": r"""
        struct S {unsigned int a:3,b:5; signed int c:7;int tail;};
        struct S s={5,17,-11,9};
        int main(void) {
            return s.a!=5 || s.b!=17 || s.c!=-11
                || s.tail!=9 || sizeof(s)!=8;
        }
    """,
    "automatic_positional": r"""
        struct S {unsigned int a:3,b:5; signed int c:7;int tail;};
        int main(void) {
            struct S s={3,27,-7,123};
            return s.a!=3 || s.b!=27 || s.c!=-7
                || s.tail!=123 || sizeof(s)!=8;
        }
    """,
    "global_sparse_designation": r"""
        struct S {unsigned int a:3,b:5,c:12;};
        struct S s={.c=1599,.a=6};
        int main(void) {
            return s.a!=6 || s.b!=0 || s.c!=1599
                || sizeof(s)!=4;
        }
    """,
    "automatic_sparse_designation": r"""
        struct S {unsigned int a:3,b:5,c:12;};
        int main(void) {
            struct S s={.b=23,.a=3};
            return s.a!=3 || s.b!=23 || s.c!=0;
        }
    """,
    "global_member_after_char": r"""
        struct S {char marker;unsigned int a:3,b:5;int tail;};
        struct S s={.marker='Z',.a=5,.b=19,.tail=23};
        int main(void) {
            return s.marker!='Z' || s.a!=5 || s.b!=19
                || s.tail!=23 || sizeof(s)!=8;
        }
    """,
    "automatic_member_after_char": r"""
        struct S {char marker;unsigned int a:3,b:5;int tail;};
        int main(void) {
            struct S s={'A',4,28,99};
            return s.marker!='A' || s.a!=4 || s.b!=28
                || s.tail!=99 || sizeof(s)!=8;
        }
    """,
    "global_nested_braced": r"""
        struct Flags {unsigned int a:4,b:4;};
        struct Box {int id; struct Flags flags;int tail;};
        struct Box b={.flags={.b=11,.a=3},.id=9,.tail=31};
        int main(void) {
            return b.id!=9 || b.flags.a!=3 || b.flags.b!=11
                || b.tail!=31 || sizeof(b)!=12;
        }
    """,
    "automatic_nested_braced": r"""
        struct Flags {unsigned int a:4,b:4;};
        struct Box {int id; struct Flags flags;};
        int main(void) {
            struct Box b={5,{7,9}};
            return b.id!=5 || b.flags.a!=7 || b.flags.b!=9;
        }
    """,
    "global_deep_designated_fields": r"""
        struct Flags {unsigned int a:4,b:5;};
        struct Box {int id; struct Flags flags;};
        struct Box b={.flags.b=17,.id=11,.flags.a=9};
        int main(void) {
            return b.id!=11 || b.flags.a!=9 || b.flags.b!=17;
        }
    """,
    "automatic_deep_designated_fields": r"""
        struct Flags {unsigned int a:4,b:5;};
        struct Box {int id; struct Flags flags;};
        int main(void) {
            struct Box b={.flags.b=21,.flags.a=7,.id=13};
            return b.id!=13 || b.flags.a!=7 || b.flags.b!=21;
        }
    """,
    "global_array_deep_designators": r"""
        struct Bits {unsigned int a:3,b:5;};
        struct Bits xs[3]={[2].b=23,[0].a=7,[2].a=3};
        int main(void) {
            return xs[0].a!=7 || xs[0].b!=0 || xs[1].a!=0
                || xs[2].a!=3 || xs[2].b!=23;
        }
    """,
    "automatic_array_initializer": r"""
        struct Bits {unsigned int a:3,b:5;};
        int main(void) {
            struct Bits xs[2]={{3,17},{6,22}};
            return xs[0].a!=3 || xs[0].b!=17
                || xs[1].a!=6 || xs[1].b!=22;
        }
    """,
    "global_union_bitfield": r"""
        union Choice {unsigned int a:3; long value;};
        union Choice c={.a=5};
        int main(void) {return c.a!=5 || sizeof(c)!=8;}
    """,
    "automatic_union_bitfield": r"""
        union Choice {signed int a:5; long value;};
        int main(void) {
            union Choice c={.a=-11};
            return c.a!=-11 || sizeof(c)!=8;
        }
    """,
    "global_signed_and_full_width": r"""
        struct S {signed int a:5;unsigned int b:27;};
        struct S s={.a=-7,.b=1048577};
        struct Full {unsigned int word:32;};
        struct Full f={.word=0xABCD0123u};
        int main(void) {
            return s.a!=-7 || s.b!=1048577
                || f.word!=0xABCD0123u || sizeof(s)!=4;
        }
    """,
    "automatic_compound_literal": r"""
        struct S {unsigned int a:3,b:5;};
        int main(void) {
            struct S *p=&(struct S){.a=5,.b=13};
            return p->a!=5 || p->b!=13;
        }
    """,
    "static_compound_literal": r"""
        struct S {unsigned int a:3,b:5;};
        struct S *p=&(struct S){.a=3,.b=19};
        int main(void) {
            if(p->a!=3 || p->b!=19) return 1;
            p->a=6;
            return p->a!=6 || p->b!=19;
        }
    """,
    "global_nested_with_pointer": r"""
        struct Bits {unsigned int a:3,b:5;};
        struct Holder {struct Bits bits;int *ptr;};
        int external=41;
        struct Holder h={.bits={.a=3,.b=17},.ptr=&external};
        int main(void) {
            return h.bits.a!=3 || h.bits.b!=17 || *h.ptr!=41;
        }
    """,
    "initializers_then_runtime_updates": r"""
        struct Bits {unsigned int a:4,b:4;};
        struct Bits static_bits={.a=3,.b=13};
        int main(void) {
            struct Bits automatic={.a=2,.b=7};
            static_bits.a++;
            automatic.b+=3;
            return static_bits.a!=4 || static_bits.b!=13
                || automatic.a!=2 || automatic.b!=10;
        }
    """,
    "global_word_boundary": r"""
        struct Bits {unsigned int first:30,second:4,third:5;};
        struct Bits bits={.first=123456789,.second=11,.third=23};
        int main(void) {
            return bits.first!=123456789 || bits.second!=11
                || bits.third!=23 || sizeof(bits)!=8;
        }
    """,
}


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, capture_output=True,
                          check=False, timeout=60)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path,
                        default=ROOT / "target/debug/mini-c-cranelift")
    args = parser.parse_args()
    backend = args.backend.resolve()
    with tempfile.TemporaryDirectory(prefix="mini-c-bitfield-init-") as dirname:
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
                raise AssertionError(f"{name}: expected native ELF")
            print(f"{name}: GCC/Mini-C bit-field initializer execution PASS")
    print(f"Bit-field C99 aggregate initializer native suite: {len(CASES)} cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
