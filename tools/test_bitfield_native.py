#!/usr/bin/env python3
"""Native GCC-vs-Mini-C C99 named 32-bit int bit-field execution comparisons."""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "unsigned_adjacent": r"""
        struct S {unsigned int a:3,b:5,c:7;};
        int main(void) {
            struct S x;
            x.a=5; x.b=17; x.c=72;
            return x.a!=5 || x.b!=17 || x.c!=72 || sizeof(x)!=4;
        }
    """,
    "signed_negative": r"""
        struct S {signed int neg:5; signed int next:9;};
        int main(void) {
            struct S s;
            s.neg=-7; s.next=-183;
            return s.neg!=-7 || s.next!=-183 || sizeof(s)!=4;
        }
    """,
    "signed_positive": r"""
        struct S {signed int x:7;unsigned int y:13;};
        int main(void) {
            struct S s;
            s.x=53;s.y=700;
            return s.x!=53 || s.y!=700 || sizeof(s)!=4;
        }
    """,
    "cross_unit_boundary": r"""
        struct S {unsigned int first:30,second:4,last:5;};
        int main(void) {
            struct S x;
            x.first=999999;x.second=13;x.last=23;
            return x.first!=999999 || x.second!=13
                || x.last!=23 || sizeof(x)!=8;
        }
    """,
    "full_width": r"""
        struct S {unsigned int mask:32; signed int full:32;};
        int main(void) {
            struct S s;
            s.mask=0xF2345678u;s.full=-123456789;
            return s.mask!=0xF2345678u || s.full!=-123456789
                || sizeof(s)!=8;
        }
    """,
    "after_char_preserves_byte": r"""
        struct S {char prefix;unsigned int a:3,b:5;int tail;};
        int main(void) {
            struct S s;
            s.prefix='Q';s.a=5;s.b=17;s.tail=19;
            return s.prefix!='Q' || s.a!=5 || s.b!=17
                || s.tail!=19 || sizeof(s)!=8;
        }
    """,
    "ordinary_member_after_bits": r"""
        struct S {unsigned int a:3,b:5;int tail;unsigned int c:6;};
        int main(void) {
            struct S s;
            s.a=5;s.b=17;s.tail=29;s.c=43;
            return s.a!=5 || s.b!=17 || s.tail!=29
                || s.c!=43 || sizeof(s)!=12;
        }
    """,
    "pointer_arrow": r"""
        struct S {unsigned int a:3,b:5;};
        int main(void) {
            struct S s;
            struct S *p=&s;
            p->a=3;p->b=23;
            return p->a!=3 || p->b!=23;
        }
    """,
    "global_zero_storage": r"""
        struct S {unsigned int a:3,b:5;int count;};
        struct S global;
        int main(void) {
            if(global.a!=0 || global.b!=0 || global.count!=0) return 1;
            global.a=6;global.b=9;global.count=17;
            return global.a!=6 || global.b!=9 || global.count!=17;
        }
    """,
    "compound_assignment": r"""
        struct S {unsigned int a:5,b:5;};
        int main(void) {
            struct S s;
            s.a=7;s.b=15;
            s.a+=9;s.b-=4;
            s.a*=2;
            return s.a!=0 || s.b!=11;
        }
    """,
    "postfix_prefix": r"""
        struct S {unsigned int a:5,b:5;};
        int main(void) {
            struct S s;
            s.a=17;s.b=9;
            int old=s.a++;
            int now=++s.b;
            return old!=17 || now!=10 || s.a!=18 || s.b!=10;
        }
    """,
    "bitfield_assignment_value_narrowing": r"""
        struct S {unsigned int a:3; signed int b:5;};
        int main(void) {
            struct S s;
            int first=(s.a=11);
            int second=(s.b=7);
            return first!=3 || s.a!=3 || second!=7 || s.b!=7;
        }
    """,
    "copy_struct_bits": r"""
        struct S {unsigned int a:3,b:5;int tail;};
        int main(void) {
            struct S original;
            struct S copy;
            original.a=5;original.b=17;original.tail=42;
            copy=original;
            return copy.a!=5 || copy.b!=17 || copy.tail!=42;
        }
    """,
    "union_signed_bits": r"""
        union U {signed int x:7;unsigned int y:3;long z;};
        int main(void) {
            union U u;
            u.x=-21;
            return u.x!=-21 || sizeof(u)!=8;
        }
    """,
    "typedef_integer_bitfield": r"""
        typedef unsigned int Word;
        struct S {Word flags:6,count:5;};
        int main(void) {
            struct S s;
            s.flags=37;s.count=23;
            return s.flags!=37 || s.count!=23;
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
    with tempfile.TemporaryDirectory(prefix="mini-c-bitfield-") as dirname:
        base = Path(dirname)
        for name, source in CASES.items():
            source_file = base / f"{name}.c"
            obj = base / f"{name}.o"
            gcc = base / f"{name}.gcc"
            mini = base / f"{name}.mini"
            source_file.write_text(source, encoding="utf-8")
            for command in (
                ["cc", "-std=c99", "-pedantic-errors", "-O0", str(source_file), "-o", str(gcc)],
                [str(backend), "--emit-obj", str(obj), str(source_file)],
                ["cc", str(obj), "-o", str(mini)],
                [str(gcc)],
                [str(mini)],
            ):
                result = run(command)
                if result.returncode or result.stdout or result.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} code={result.returncode} "
                        f"stdout={result.stdout!r} stderr={result.stderr!r}"
                    )
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: missing native ELF")
            print(f"{name}: GCC/Mini-C bit-field native execution PASS")
    print(f"C99 named bit-field native suite: {len(CASES)} cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
