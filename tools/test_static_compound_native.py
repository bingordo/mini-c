#!/usr/bin/env python3
"""C99 file-scope compound literal static-storage GCC/Mini-C native differential."""
from __future__ import annotations

import argparse
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

CASES = {
    "global_inferred_array": r"""
        int *numbers=(int[]){2,5,9};
        int main(void) {
            return numbers[0]!=2 || numbers[1]!=5 || numbers[2]!=9;
        }
    """,
    "global_scalar_address": r"""
        int *number=&(int){7};
        int main(void) {
            if (*number!=7) return 1;
            *number=11;
            return *number!=11;
        }
    """,
    "global_struct_address": r"""
        struct Pair {int x;int y;};
        struct Pair *pair=&(struct Pair){.y=8,.x=3};
        int main(void) {return pair->x!=3 || pair->y!=8;}
    """,
    "two_distinct_compounds": r"""
        int *a=(int[]){3,4};
        int *b=(int[]){3,4};
        int main(void) {
            if (a==b) return 1;
            a[0]=9;
            return b[0]!=3 || a[0]!=9;
        }
    """,
    "static_sparse_array": r"""
        int *values=(int[]){[8]=4,[2]=7};
        int main(void) {
            return values[8]!=4 || values[2]!=7
                || values[0]!=0 || values[7]!=0;
        }
    """,
    "compound_member_addend": r"""
        struct S {int first;int second;long third;};
        int *member=&((struct S){.second=19,.first=3,.third=7}).second;
        int main(void) {
            if (*member!=19) return 1;
            *member=23;
            return *member!=23;
        }
    """,
    "global_aggregate_pointer": r"""
        struct Holder {int *data;int code;};
        struct Holder holder={.data=(int[]){1,4,7},.code=11};
        int main(void) {
            return holder.data[0]!=1 || holder.data[2]!=7
                || holder.code!=11;
        }
    """,
    "array_of_pointer_compounds": r"""
        int *items[]={(int[]){2,4},(int[]){7,8}};
        int main(void) {
            return items[0]==items[1] || items[0][1]!=4 || items[1][0]!=7;
        }
    """,
    "nested_compound_relocations": r"""
        int target=13;
        int **table=(int*[]){&target,&(int){17}};
        int main(void) {
            return *table[0]!=13 || *table[1]!=17
                || table[0]==table[1];
        }
    """,
    "static_char_array": r"""
        char *word=(char[]){'O','K',0};
        int main(void) {
            return word[0]!='O' || word[1]!='K' || word[2]!=0;
        }
    """,
    "compound_scalar_in_aggregate": r"""
        struct S {int *first;int *second;};
        struct S p={.second=&(int){31},.first=&(int){17}};
        int main(void) {
            return *p.first!=17 || *p.second!=31 || p.first==p.second;
        }
    """,
    "static_lifetime_across_functions": r"""
        int *ref=(int[]){3,4};
        void change(void) { ref[0]=17; }
        int read(void) { return ref[0]; }
        int main(void) {
            change();
            return read()!=17 || ref[1]!=4;
        }
    """,
    "static_anonymous_array_pointer": r"""
        int (*matrix)[3]=&(int[3]){6,7,8};
        int main(void) {
            return (*matrix)[0]!=6 || (*matrix)[2]!=8;
        }
    """,
    "relocation_to_member_in_aggregate": r"""
        struct Pair {int a;int b;};
        struct Ref {int *ptr;int flag;};
        struct Ref root={.flag=7,.ptr=&((struct Pair){.a=3,.b=9}).b};
        int main(void) {
            return root.flag!=7 || *root.ptr!=9;
        }
    """,
    "static_inline_anonymous_struct_member": r"""
        int *field=&((struct {int a;int b;}){.a=3,.b=8}).b;
        int main(void) {
            if (*field!=8) return 1;
            *field=11;
            return *field!=11;
        }
    """,
    "static_inline_anonymous_union_member": r"""
        long *field=&((union {int a;long b;}){.b=17}).b;
        int main(void) {return *field!=17;}
    """,
    "static_inline_nested_member_addend": r"""
        int *value=&((struct {
            struct {int x;int y;} nested;
            int tag;
        }){.nested={.x=2,.y=9},.tag=4}).nested.y;
        int main(void) {return *value!=9;}
    """,
    "static_inline_anonymous_distinct_objects": r"""
        int *first=&((struct {int x;}){.x=7}).x;
        int *second=&((struct {int x;}){.x=7}).x;
        int main(void) {
            if (first==second) return 1;
            *first=11;
            return *second!=7 || *first!=11;
        }
    """,
    "static_named_tag_created_by_compound": r"""
        int *p=&((struct Defined {int x;int y;}){.x=3,.y=5}).y;
        struct Defined other={7,9};
        int main(void) {
            return *p!=5 || other.x!=7 || other.y!=9;
        }
    """,
    "static_inline_aggregate_pointer_relocation": r"""
        struct Box {int *p;int flag;};
        struct Box box={
            .p=&((struct {int x;int y;}){.x=3,.y=13}).y,
            .flag=7
        };
        int main(void) {
            return *box.p!=13 || box.flag!=7;
        }
    """,
    "static_inline_named_tag_sizeof": r"""
        int size=sizeof(struct Defined {int x;long y;});
        struct Defined value={3,5};
        int main(void) {
            return size!=16 || value.x!=3 || value.y!=5;
        }
    """,
    "static_inline_aggregate_with_existing_member": r"""
        struct Inner {long value;};
        long *value=&((struct {
            int marker;
            struct Inner nested;
        }){.marker=4,.nested={.value=19}}).nested.value;
        int main(void) {return *value!=19;}
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
    with tempfile.TemporaryDirectory(prefix="mini-c-static-compound-") as tmpdir:
        temp = Path(tmpdir)
        for name, source in CASES.items():
            path = temp / f"{name}.c"
            obj = temp / f"{name}.o"
            gcc = temp / f"{name}.gcc"
            mini = temp / f"{name}.mini"
            path.write_text(source, encoding="utf-8")
            commands = (
                ["cc", "-std=c99", "-pedantic-errors", "-O0", str(path), "-o", str(gcc)],
                [str(backend), "--emit-obj", str(obj), str(path)],
                ["cc", str(obj), "-o", str(mini)],
                [str(gcc)],
                [str(mini)],
            )
            for command in commands:
                result = run(command)
                if result.returncode != 0 or result.stdout or result.stderr:
                    raise AssertionError(
                        f"{name}: {' '.join(command)} code={result.returncode}; "
                        f"stdout={result.stdout!r} stderr={result.stderr!r}"
                    )
            if not obj.read_bytes().startswith(b"\x7fELF"):
                raise AssertionError(f"{name}: expected native ELF object")
            print(f"{name}: GCC/Mini-C static compound execution PASS")
    print(f"C99 file-scope compound literals: {len(CASES)} native cases passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
