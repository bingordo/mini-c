#!/usr/bin/env python3
"""Compile, link and execute original C99 designated-initializer programs with GCC and Mini-C.

This F3 suite covers single-level and nested mixed field/array designators,
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
        struct Outer { int count; int inner[3]; int tail; };
        int main(void) {
            struct Outer o={.inner={[2]=8,[0]=2},.count=4};
            return o.count!=4 || o.inner[0]!=2 || o.inner[1]!=0
                || o.inner[2]!=8 || o.tail!=0;
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
    "deep_struct_auto": r"""
        struct Record { int data[3]; int tag; };
        int main(void) {
            struct Record r={.data[2]=9,.data[0]=3,.tag=7};
            return r.data[0]!=3 || r.data[1]!=0 || r.data[2]!=9 || r.tag!=7;
        }
    """,
    "deep_multidimensional_array": r"""
        int main(void) {
            int a[3][4]={[2][3]=9,[0][1]=5,[2][0]=7};
            return a[2][3]!=9 || a[0][1]!=5 || a[2][0]!=7
                || a[2][1]!=0 || a[1][3]!=0;
        }
    """,
    "deep_global_array_of_structs": r"""
        struct Item { int id; int flags[2]; };
        struct Item items[4]={
            [2].id=12,
            [2].flags[1]=8,
            [0].flags[0]=3,
        };
        int main(void) {
            return items[2].id!=12 || items[2].flags[0]!=0
                || items[2].flags[1]!=8 || items[0].flags[0]!=3
                || items[1].id!=0 || items[3].id!=0;
        }
    """,
    "deep_inferred_first_bound": r"""
        int values[][2]={[4][1]=7,[1][0]=2};
        int main(void) {
            return sizeof(values)!=5*2*sizeof(int)
                || values[4][1]!=7 || values[1][0]!=2
                || values[4][0]!=0 || values[0][0]!=0;
        }
    """,
    "deep_global_pointer_relocation": r"""
        int target=23;
        struct Holder { int *items[2]; int tag; };
        struct Holder holder={.items[1]=&target,.tag=4};
        int main(void) {
            return holder.tag!=4 || holder.items[0]!=0
                || *holder.items[1]!=23;
        }
    """,
    "deep_dynamic_side_effects_once": r"""
        int calls;
        int side(void) { calls++; return calls*3; }
        struct Object { int values[3]; int tag; };
        int main(void) {
            struct Object a={.values[2]=side(),.tag=side(),.values[0]=side()};
            return calls!=3 || a.values[0]+a.values[2]+a.tag!=18
                || a.values[1]!=0;
        }
    """,
    "deep_union_array_member": r"""
        union U { int numbers[3]; long wide; };
        int main(void) {
            union U u={.numbers[2]=4,.numbers[0]=1};
            return u.numbers[0]!=1 || u.numbers[1]!=0 || u.numbers[2]!=4;
        }
    """,
    "deep_three_level_path": r"""
        struct Grid { int cells[2][3]; int tag; };
        int main(void) {
            struct Grid g={.cells[1][2]=13,.cells[0][1]=8,.tag=4};
            return g.cells[1][2]!=13 || g.cells[0][1]!=8
                || g.cells[1][1]!=0 || g.tag!=4;
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
