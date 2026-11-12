#!/usr/bin/env python3
"""Compile, link and execute original C99 designated-initializer programs with GCC and Mini-C.

This bounded F3 slice supports single-level field and array designators,
sparse omitted subobjects, and recursively braced nested aggregates.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "sparse_auto_array_and_cursor": r"""
        int main(void) {
            int numbers[9] = {[4]=7, 8, [1]=3};
            return numbers[0]!=0 || numbers[1]!=3
                || numbers[2]!=0 || numbers[4]!=7
                || numbers[5]!=8 || numbers[8]!=0;
        }
    """,
    "struct_out_of_order_and_cursor": r"""
        struct S { int a; int b; int c; int d; };
        int main(void) {
            struct S s={.c=5,.a=1,2};
            return s.a!=1 || s.b!=2 || s.c!=5 || s.d!=0;
        }
    """,
    "global_sparse_inferred_array": r"""
        int global[]={[9]=7,[2]=3,4};
        int main(void) {
            return sizeof(global)!=10*sizeof(int)
                || global[0]!=0 || global[2]!=3 || global[3]!=4
                || global[9]!=7 || global[8]!=0;
        }
    """,
    "nested_brace_designators": r"""
        struct Inner { int a; int b; int c; };
        struct Outer { int count; struct Inner inner; int tail; };
        int main(void) {
            struct Outer o={.inner={.c=8,.a=2},.count=4};
            return o.count!=4 || o.inner.a!=2 || o.inner.b!=0
                || o.inner.c!=8 || o.tail!=0;
        }
    """,
    "union_selected_member": r"""
        union U { int small; long large; };
        union U global={.large=19};
        int main(void) {
            union U local={.large=23};
            return global.large!=19 || local.large!=23;
        }
    """,
    "global_pointer_relocation": r"""
        int target=23;
        struct Ref { int count; int *pointer; };
        struct Ref item={.pointer=&target,.count=4};
        int main(void) { return item.count!=4 || *item.pointer!=23; }
    """,
    "sparse_side_effects_once": r"""
        int count;
        int bump(void) { count++; return count; }
        int main(void) {
            int a[5]={[3]=bump(),[0]=bump(),[1]=bump()};
            return count!=3 || a[0]+a[1]+a[3]!=6
                || a[2]!=0 || a[4]!=0;
        }
    """,
    "array_of_struct_with_brace_designators": r"""
        struct Entry { int key; int value; };
        struct Entry entries[5] = {
            [3] = {.value=12,.key=4},
            [1] = {.key=8,.value=9},
        };
        int main(void) {
            return entries[3].key!=4 || entries[3].value!=12
                || entries[1].key!=8 || entries[1].value!=9
                || entries[0].key!=0 || entries[4].value!=0;
        }
    """,
}


def execute(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, text=True, capture_output=True,
                          check=False, timeout=45)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path,
                        default=ROOT / "target/debug/mini-c-cranelift")
    args = parser.parse_args()
    backend = args.backend.resolve()
    with tempfile.TemporaryDirectory(prefix="mini-c-designated-") as tmp:
        base = Path(tmp)
        for name, source in CASES.items():
            path = base / f"{name}.c"
            obj = base / f"{name}.o"
            gcc = base / f"{name}.gcc"
            mini = base / f"{name}.mini"
            path.write_text(source, encoding="utf-8")
            commands = [
                ["cc", "-std=c99", "-pedantic-errors", "-O0", str(path), "-o", str(gcc)],
                [str(backend), "--emit-obj", str(obj), str(path)],
                ["cc", str(obj), "-o", str(mini)],
                [str(gcc)],
                [str(mini)],
            ]
            for command in commands:
                result = execute(command)
                if result.returncode != 0 or result.stdout or result.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} code={result.returncode}; "
                        f"stdout={result.stdout!r} stderr={result.stderr!r}"
                    )
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: missing ELF object")
            print(f"{name}: GCC/Mini-C designated initializer execution PASS")
        print(f"F3 designated initializer native: {len(CASES)} cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
